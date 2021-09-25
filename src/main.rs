use dotenv::dotenv;
use futures_util::StreamExt;
use pektin::persistence::{get_rrset, QueryResponse};
use pektin::{load_env, PektinError};
use redis::Client;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, UdpSocket};
use trust_dns_proto::op::{Edns, Message, MessageType, ResponseCode};
use trust_dns_proto::rr::Record;
use trust_dns_proto::udp::UdpStream;
use trust_dns_proto::xfer::{BufDnsStreamHandle, SerialMessage};
use trust_dns_proto::DnsStreamHandle;

const D_BIND_ADDRESS: &'static str = "0.0.0.0";
const D_BIND_PORT: &'static str = "53";
const D_REDIS_URI: &'static str = "redis://pektin-redis:6379";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv().ok();

    println!("Started Pektin with these globals:");
    let redis_uri = load_env(D_REDIS_URI, "REDIS_URI");
    let bind_address = load_env(D_BIND_ADDRESS, "BIND_ADDRESS");
    let bind_port = load_env(D_BIND_PORT, "BIND_PORT");
    let redis_retry_seconds = 3;

    let redis_client = Client::open(redis_uri)?;
    let mut retry_count = 0;
    loop {
        match redis_client.get_async_connection().await {
            Ok(_) => break,
            Err(_) => {
                retry_count += 1;
                eprintln!(
                    "Could not connect to redis, retrying in {} seconds... (retry {})",
                    redis_retry_seconds, retry_count
                );
                std::thread::sleep(Duration::from_secs(redis_retry_seconds));
            }
        }
    }
    let redis_client = Arc::new(redis_client);
    let udp_client = redis_client.clone();

    // TODO also listen for TCP requests
    let udp_socket = UdpSocket::bind(format!("{}:{}", bind_address, bind_port)).await?;

    // see trust_dns_server::server::ServerFuture::register_socket
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
            let req_client = udp_client.clone();
            tokio::spawn(async move {
                handle_request(message, udp_handle, req_client).await;
            });
        }
    });

    // TODO the same for TCP
    // TODO then join the UDP and TCP handles
    if let Err(e) = udp_join_handle.await {
        eprintln!("UDP handling task returned error: {}", e)
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
