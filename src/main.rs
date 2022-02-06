use dotenv::dotenv;
use futures_util::{future, StreamExt};
use pektin_common::deadpool_redis::Pool;
use pektin_common::load_env;
use pektin_common::proto::iocompat::AsyncIoTokioAsStd;
use pektin_common::proto::op::Message;
use pektin_common::proto::tcp::TcpStream;
use pektin_common::proto::udp::UdpStream;
use pektin_common::proto::xfer::{BufDnsStreamHandle, SerialMessage};
use pektin_common::proto::DnsStreamHandle;
use pektin_server::{process_request, PektinResult};
use std::net::Ipv6Addr;
use std::time::Duration;
use tokio::net::{TcpListener, UdpSocket};
use trust_dns_server::server::TimeoutStream;

mod doh;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    pub bind_address: Ipv6Addr,
    pub bind_port: u16,
    pub redis_hostname: String,
    pub redis_username: String,
    pub redis_password: String,
    pub redis_port: u16,
    pub redis_retry_seconds: u64,
    pub tcp_timeout_seconds: u64,
    pub use_doh: bool,
    pub doh_bind_address: Ipv6Addr,
    pub doh_bind_port: u16,
}

impl Config {
    pub fn from_env() -> PektinResult<Self> {
        Ok(Self {
            bind_address: load_env("::", "BIND_ADDRESS", false)?
                .parse()
                .map_err(|_| {
                    pektin_common::PektinCommonError::InvalidEnvVar("BIND_ADDRESS".into())
                })?,
            bind_port: load_env("53", "BIND_PORT", false)?
                .parse()
                .map_err(|_| pektin_common::PektinCommonError::InvalidEnvVar("BIND_PORT".into()))?,
            redis_hostname: load_env("pektin-redis", "REDIS_HOSTNAME", false)?,
            redis_port: load_env("6379", "REDIS_PORT", false)?
                .parse()
                .map_err(|_| {
                    pektin_common::PektinCommonError::InvalidEnvVar("REDIS_PORT".into())
                })?,
            redis_username: load_env("r-pektin-server", "REDIS_USERNAME", false)?,
            redis_password: load_env("", "REDIS_PASSWORD", true)?,
            redis_retry_seconds: load_env("1", "REDIS_RETRY_SECONDS", false)?
                .parse()
                .map_err(|_| {
                    pektin_common::PektinCommonError::InvalidEnvVar("REDIS_RETRY_SECONDS".into())
                })?,
            tcp_timeout_seconds: load_env("3", "TCP_TIMEOUT_SECONDS", false)?
                .parse()
                .map_err(|_| {
                    pektin_common::PektinCommonError::InvalidEnvVar("TCP_TIMEOUT_SECONDS".into())
                })?,
            use_doh: load_env("true", "USE_DOH", false).unwrap() == "true",
            doh_bind_port: load_env("80", "DOH_BIND_PORT", false)?
                .parse()
                .map_err(|_| {
                    pektin_common::PektinCommonError::InvalidEnvVar("DOH_BIND_PORT".into())
                })?,
            doh_bind_address: load_env("::", "DOH_BIND_ADDRESS", false)?
                .parse()
                .map_err(|_| {
                    pektin_common::PektinCommonError::InvalidEnvVar("DOH_BIND_ADDRESS".into())
                })?,
        })
    }
}

#[tokio::main]
async fn main() -> PektinResult<()> {
    dotenv().ok();

    println!("Started Pektin with these globals:");
    let config = Config::from_env()?;

    let redis_pool_conf = pektin_common::deadpool_redis::Config {
        url: Some(format!(
            "redis://{}:{}@{}:{}",
            config.redis_username, config.redis_password, config.redis_hostname, config.redis_port
        )),
        connection: None,
        pool: None,
    };
    let redis_pool = redis_pool_conf.create_pool()?;

    let doh_redis_pool = redis_pool.clone();
    let doh_server = if config.use_doh {
        match doh::use_doh(
            config.doh_bind_address,
            config.doh_bind_port,
            doh_redis_pool,
        )
        .await
        {
            Ok(server) => Some(server),
            Err(e) => {
                eprintln!("Error while trying to start DOH server: {}", e);
                None
            }
        }
    } else {
        None
    };

    let udp_redis_pool = redis_pool.clone();
    let udp_socket =
        UdpSocket::bind(format!("[{}]:{}", &config.bind_address, config.bind_port)).await?;
    let udp_join_handle = tokio::spawn(async move {
        message_loop_udp(udp_socket, udp_redis_pool).await;
    });

    let tcp_redis_pool = redis_pool.clone();
    let tcp_listener =
        TcpListener::bind(format!("[{}]:{}", &config.bind_address, config.bind_port)).await?;
    let tcp_join_handle = tokio::spawn(async move {
        message_loop_tcp(tcp_listener, tcp_redis_pool).await;
    });

    match doh_server {
        Some(server) => {
            let (res1, res2, res3) = future::join3(udp_join_handle, tcp_join_handle, server).await;
            if res1.is_err() || res2.is_err() || res3.is_err() {
                eprintln!("Internal error in tokio spawn")
            }
        }
        None => {
            let (res1, res2) = future::join(udp_join_handle, tcp_join_handle).await;
            if res1.is_err() || res2.is_err() {
                eprintln!("Internal error in tokio spawn")
            }
        }
    }

    Ok(())
}

async fn message_loop_udp(socket: UdpSocket, redis_pool: Pool) {
    // see trust_dns_server::server::ServerFuture::register_socket
    let (mut udp_stream, udp_handle) =
        UdpStream::with_bound(socket, ([127, 255, 255, 254], 0).into());
    while let Some(message) = udp_stream.next().await {
        let message = match message {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Error receiving UDP message: {}", e);
                continue;
            }
        };

        let src_addr = message.addr();
        let udp_handle = udp_handle.with_remote_addr(src_addr);
        let req_redis_pool = redis_pool.clone();
        tokio::spawn(async move {
            handle_request_udp_tcp(message, udp_handle, req_redis_pool).await;
        });
    }
}

async fn message_loop_tcp(listener: TcpListener, redis_pool: Pool) {
    // see trust_dns_server::server::ServerFuture::register_listener
    loop {
        let tcp_stream = match listener.accept().await {
            Ok((t, _)) => t,
            Err(e) => {
                eprintln!("Error creating a new TCP stream: {}", e);
                continue;
            }
        };

        let req_redis_pool = redis_pool.clone();
        tokio::spawn(async move {
            let src_addr = tcp_stream.peer_addr().unwrap();
            let (tcp_stream, tcp_handle) =
                TcpStream::from_stream(AsyncIoTokioAsStd(tcp_stream), src_addr);
            // TODO maybe make this configurable via environment variable?
            let mut timeout_stream = TimeoutStream::new(tcp_stream, Duration::from_secs(3));

            while let Some(message) = timeout_stream.next().await {
                let message = match message {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("Error receiving TCP message: {}", e);
                        return;
                    }
                };

                handle_request_udp_tcp(message, tcp_handle.clone(), req_redis_pool.clone()).await;
            }
        });
    }
}

async fn handle_request_udp_tcp(
    msg: SerialMessage,
    stream_handle: BufDnsStreamHandle,
    redis_pool: Pool,
) {
    let message = match msg.to_message() {
        Ok(m) => m,
        _ => {
            eprintln!("Could not deserialize received message");
            return;
        }
    };
    let response = process_request(message, redis_pool).await;
    send_response(msg, response, stream_handle)
}

fn send_response(query: SerialMessage, response: Message, mut stream_handle: BufDnsStreamHandle) {
    let response_bytes = match response.to_vec() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Could not serialize response: {}", e);
            return;
        }
    };
    let serialized_response = SerialMessage::new(response_bytes, query.addr());
    if let Err(e) = stream_handle.send(serialized_response) {
        eprintln!("Could not send response: {}", e);
    }
}
