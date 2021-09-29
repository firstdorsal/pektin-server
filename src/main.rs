use dotenv::dotenv;
use futures_util::{future, StreamExt};
use pektin_common::load_env;
use pektin_server::persistence::{get_rrset, QueryResponse};
use pektin_server::{PektinError, PektinResult};
use redis::Client;
use std::error::Error;
use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, UdpSocket};
use trust_dns_proto::iocompat::AsyncIoTokioAsStd;
use trust_dns_proto::op::{Edns, Message, MessageType, ResponseCode};
use trust_dns_proto::rr::Record;
use trust_dns_proto::tcp::TcpStream;
use trust_dns_proto::udp::UdpStream;
use trust_dns_proto::xfer::{BufDnsStreamHandle, SerialMessage};
use trust_dns_proto::DnsStreamHandle;
use trust_dns_server::server::TimeoutStream;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    pub bind_address: Ipv6Addr,
    pub bind_port: u16,
    pub redis_uri: String,
    pub redis_retry_seconds: u64,
    pub tcp_timeout_seconds: u64,
}

impl Config {
    pub fn from_env() -> PektinResult<Self> {
        Ok(Self {
            bind_address: load_env("::", "BIND_ADDRESS")?.parse().map_err(|_| {
                pektin_common::PektinCommonError::InvalidEnvVar("BIND_ADDRESS".into())
            })?,
            bind_port: load_env("53", "BIND_PORT")?
                .parse()
                .map_err(|_| pektin_common::PektinCommonError::InvalidEnvVar("BIND_PORT".into()))?,
            redis_uri: load_env("redis://pektin-redis:6379", "REDIS_URI")?,
            redis_retry_seconds: load_env("1", "REDIS_RETRY_SECONDS")?.parse().map_err(|_| {
                pektin_common::PektinCommonError::InvalidEnvVar("REDIS_RETRY_SECONDS".into())
            })?,
            tcp_timeout_seconds: load_env("3", "TCP_TIMEOUT_SECONDS")?.parse().map_err(|_| {
                pektin_common::PektinCommonError::InvalidEnvVar("TCP_TIMEOUT_SECONDS".into())
            })?,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv().ok();

    println!("Started Pektin with these globals:");
    let config = Config::from_env()?;

    let redis_client = Client::open(config.redis_uri.clone())?;
    let mut retry_count = 0;
    loop {
        match redis_client.get_async_connection().await {
            Ok(_) => break,
            Err(_) => {
                retry_count += 1;
                eprintln!(
                    "Could not connect to redis, retrying in {} seconds... (retry {})",
                    config.redis_retry_seconds, retry_count
                );
                std::thread::sleep(Duration::from_secs(config.redis_retry_seconds));
            }
        }
    }
    let redis_client = Arc::new(redis_client);

    // see trust_dns_server::server::ServerFuture::register_socket
    let udp_redis_client = redis_client.clone();
    let udp_socket =
        UdpSocket::bind(format!("[{}]:{}", &config.bind_address, config.bind_port)).await?;
    let (mut udp_stream, udp_handle) =
        UdpStream::with_bound(udp_socket, ([127, 255, 255, 254], 0).into());
    let udp_join_handle = tokio::spawn(async move {
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
            let req_redis_client = udp_redis_client.clone();
            tokio::spawn(async move {
                handle_request(message, udp_handle, req_redis_client).await;
            });
        }
    });

    // see trust_dns_server::server::ServerFuture::register_listener
    let tcp_redis_client = redis_client.clone();
    let tcp_listener =
        TcpListener::bind(format!("[{}]:{}", &config.bind_address, config.bind_port)).await?;
    let tcp_join_handle = tokio::spawn(async move {
        loop {
            let tcp_stream = match tcp_listener.accept().await {
                Ok((t, _)) => t,
                Err(e) => {
                    eprintln!("Error creating a new TCP stream: {}", e);
                    continue;
                }
            };

            let req_redis_client = tcp_redis_client.clone();
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

                    handle_request(message, tcp_handle.clone(), req_redis_client.clone()).await;
                }
            });
        }
    });

    let (res1, res2) = future::join(udp_join_handle, tcp_join_handle).await;
    if res1.is_err() || res2.is_err() {
        eprintln!("Internal error in tokio spawn")
    }

    Ok(())
}

async fn handle_request(
    msg: SerialMessage,
    mut stream_handle: BufDnsStreamHandle,
    redis_client: Arc<Client>,
) {
    // TODO: better error logging (use https://git.sr.ht/~mvforell/ddc-ci/tree/master/item/src/daemon_data.rs#L90 ?)

    let message = match msg.to_message() {
        Ok(m) => m,
        _ => {
            eprintln!("Could not deserialize received message");
            return;
        }
    };

    // TODO if m.queries().len() == 0

    let mut response = Message::new();
    response.set_id(message.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(message.op_code());
    response.set_recursion_desired(message.recursion_desired());
    response.set_recursion_available(false);
    response.set_authoritative(true);

    // TODO: check how to handle wildcards according to the relevant RFCs
    // (does a.b.example.com match *.example.com?)
    let q = &message.queries()[0];
    let res = match redis_client.get_async_connection().await {
        Ok(mut con) => get_rrset(&mut con, q.name(), q.query_type()).await,
        Err(_) => Err(PektinError::NoRedisConnection),
    };
    if let Ok(redis_response) = res {
        let rr_set = match redis_response {
            QueryResponse::Empty => {
                // TODO: return SOA?
                response.set_response_code(ResponseCode::NXDomain);
                vec![]
            }
            QueryResponse::Definitive(def) => def.rr_set,
            QueryResponse::Wildcard(wild) => wild.rr_set,
            QueryResponse::Both { definitive, .. } => definitive.rr_set,
        };
        for record in rr_set {
            let rr = Record::from_rdata(q.name().clone(), record.ttl, record.value);
            response.add_answer(rr);
        }

        let mut edns = Edns::new();
        edns.set_max_payload(4096);
        response.set_edns(edns);
    } else {
        eprintln!("{}", res.err().unwrap());
        response.set_response_code(ResponseCode::ServFail);
    }

    let response_bytes = match response.to_vec() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Could not serialize response: {}", e);
            return;
        }
    };
    let serialized_response = SerialMessage::new(response_bytes, msg.addr());
    if let Err(e) = stream_handle.send(serialized_response) {
        eprintln!("Could not send response: {}", e);
    }
}
