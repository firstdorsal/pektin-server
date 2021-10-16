use dotenv::dotenv;
use futures_util::{future, StreamExt};
use pektin_common::deadpool_redis::Pool;
use pektin_common::proto::iocompat::AsyncIoTokioAsStd;
use pektin_common::proto::op::{Edns, Message, MessageType, ResponseCode};
use pektin_common::proto::rr::{Name, Record, RecordType};
use pektin_common::proto::tcp::TcpStream;
use pektin_common::proto::udp::UdpStream;
use pektin_common::proto::xfer::{BufDnsStreamHandle, SerialMessage};
use pektin_common::proto::DnsStreamHandle;
use pektin_common::{get_authoritative_zones, load_env};
use pektin_server::persistence::{get_rrset, QueryResponse};
use pektin_server::PektinResult;
use std::net::Ipv6Addr;
use std::time::Duration;
use tokio::net::{TcpListener, UdpSocket};
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
            bind_address: load_env("::", "BIND_ADDRESS", false)?
                .parse()
                .map_err(|_| {
                    pektin_common::PektinCommonError::InvalidEnvVar("BIND_ADDRESS".into())
                })?,
            bind_port: load_env("53", "BIND_PORT", false)?
                .parse()
                .map_err(|_| pektin_common::PektinCommonError::InvalidEnvVar("BIND_PORT".into()))?,
            redis_uri: load_env("redis://pektin-redis:6379", "REDIS_URI", false)?,
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
        })
    }
}

#[tokio::main]
async fn main() -> PektinResult<()> {
    dotenv().ok();

    println!("Started Pektin with these globals:");
    let config = Config::from_env()?;

    let redis_pool_conf = pektin_common::deadpool_redis::Config {
        url: Some(config.redis_uri.clone()),
        connection: None,
        pool: None,
    };
    let redis_pool = redis_pool_conf.create_pool()?;

    // see trust_dns_server::server::ServerFuture::register_socket
    let udp_redis_pool = redis_pool.clone();
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
            let req_redis_pool = udp_redis_pool.clone();
            tokio::spawn(async move {
                handle_request(message, udp_handle, req_redis_pool).await;
            });
        }
    });

    // see trust_dns_server::server::ServerFuture::register_listener
    let tcp_redis_pool = redis_pool.clone();
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

            let req_redis_pool = tcp_redis_pool.clone();
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

                    handle_request(message, tcp_handle.clone(), req_redis_pool.clone()).await;
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

async fn handle_request(msg: SerialMessage, stream_handle: BufDnsStreamHandle, redis_pool: Pool) {
    // TODO: better error logging (use https://git.sr.ht/~mvforell/ddc-ci/tree/master/item/src/daemon_data.rs#L90 ?)

    let mut message = match msg.to_message() {
        Ok(m) => m,
        _ => {
            eprintln!("Could not deserialize received message");
            return;
        }
    };

    let mut response = Message::new();
    response.set_id(message.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(message.op_code());
    response.set_recursion_desired(message.recursion_desired());
    response.set_recursion_available(false);
    response.set_authoritative(true);

    let mut edns = Edns::new();
    edns.set_max_payload(4096);
    response.set_edns(edns);

    let mut con = match redis_pool.get().await {
        Ok(c) => c,
        Err(_) => {
            response.set_response_code(ResponseCode::ServFail);

            // copy queries
            response.add_queries(message.take_queries().into_iter());

            send_response(msg, response, stream_handle);
            return;
        }
    };

    // TODO: check how to handle wildcards according to the relevant RFCs
    // (does a.b.example.com match *.example.com?)

    // needed later for querying the SOA record if no query could be answered
    let mut zone_name = None;
    // keep track if we added an answer into the message (response.answer_count() doesn't automatically update)
    let mut answer_stored = false;

    for q in message.queries() {
        if zone_name.is_none() {
            zone_name = Some(q.name());
        }

        let res = get_rrset(&mut con, q.name(), q.query_type()).await;
        if let Ok(redis_response) = res {
            let rr_set = match redis_response {
                QueryResponse::Empty => continue,
                QueryResponse::Definitive(def) => def.rr_set,
                QueryResponse::Wildcard(wild) => wild.rr_set,
                QueryResponse::Both { definitive, .. } => definitive.rr_set,
            };
            for record in rr_set {
                let rr = Record::from_rdata(q.name().clone(), record.ttl, record.value);
                response.add_answer(rr);
            }
            answer_stored = true;
        } else {
            eprintln!("{}", res.err().unwrap());
            response.set_response_code(ResponseCode::ServFail);
            break;
        }
    }

    if !answer_stored && (response.response_code() != ResponseCode::ServFail) {
        if let Some(queried_name) = zone_name {
            if let Ok(authoritative_zones) = get_authoritative_zones(&mut con).await {
                if let Some(auth_zone) = authoritative_zones
                    .into_iter()
                    .map(|zone| {
                        Name::from_utf8(zone).expect("Name in redis is not a valid DNS name")
                    })
                    .find(|zone| zone.zone_of(&queried_name))
                {
                    let res = get_rrset(&mut con, &auth_zone, RecordType::SOA).await;
                    if let Ok(redis_response) = res {
                        let rr_set = match redis_response {
                            QueryResponse::Empty => {
                                response.set_response_code(ResponseCode::Refused);
                                vec![]
                            }
                            QueryResponse::Definitive(def) => def.rr_set,
                            QueryResponse::Wildcard(wild) => wild.rr_set,
                            QueryResponse::Both { definitive, .. } => definitive.rr_set,
                        };
                        for record in rr_set {
                            // get the name of the authoritative zone, preserving the case of the queried name
                            let mut soa_name = queried_name.clone();
                            while soa_name.num_labels() != auth_zone.num_labels() {
                                soa_name = soa_name.base_name();
                            }
                            let rr = Record::from_rdata(soa_name, record.ttl, record.value);
                            // the name is a bit misleading; this adds the record to the authority section
                            response.add_name_server(rr);
                            response.set_response_code(ResponseCode::NXDomain);
                        }
                    } else {
                        response.set_response_code(ResponseCode::ServFail);
                    }
                } else {
                    response.set_response_code(ResponseCode::Refused);
                }
            } else {
                response.set_response_code(ResponseCode::ServFail);
            }
        } else {
            // TODO is that right?
            response.set_response_code(ResponseCode::Refused);
        }
    }

    // copy queries
    response.add_queries(message.take_queries().into_iter());

    send_response(msg, response, stream_handle);
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
