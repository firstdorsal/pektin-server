use dotenv::dotenv;
use pektin::load_env;
use pektin::persistence::{get_rrset, QueryResponse};
use redis::Client;
use std::error::Error;
use std::net::UdpSocket;
use std::time::Duration;
use trust_dns_proto::op::{Edns, Message, MessageType, ResponseCode};
use trust_dns_proto::rr::Record;

const D_BIND_ADDRESS: &'static str = "0.0.0.0";
const D_BIND_PORT: &'static str = "53";
const D_REDIS_URI: &'static str = "redis://pektin-redis:6379";

const RECV_BUFSIZE: usize = 4096;

fn main() -> Result<(), Box<dyn Error>> {
    dotenv().ok();

    println!("Started Pektin with these globals:");
    let redis_uri = load_env(D_REDIS_URI, "REDIS_URI");
    let bind_address = load_env(D_BIND_ADDRESS, "BIND_ADDRESS");
    let bind_port = load_env(D_BIND_PORT, "BIND_PORT");
    let redis_retry_seconds = 3;

    let client = Client::open(redis_uri)?;
    let mut retry_count = 0;
    let mut con = loop {
        match client.get_connection() {
            Ok(c) => break c,
            Err(_) => {
                retry_count += 1;
                eprintln!(
                    "Could not connect to redis, retrying in {} seconds... (retry {})",
                    redis_retry_seconds, retry_count
                );
                std::thread::sleep(Duration::from_secs(redis_retry_seconds));
            }
        }
    };

    let socket = UdpSocket::bind(format!("{}:{}", bind_address, bind_port))?;
    let mut buf = [0; RECV_BUFSIZE];
    // TODO: better error logging (use https://git.sr.ht/~mvforell/ddc-ci/tree/master/item/src/daemon_data.rs#L90 ?)
    loop {
        let addr = match socket.recv_from(&mut buf) {
            Ok((_, a)) => a,
            Err(_) => {
                eprintln!("Could not receive message from socket");
                continue;
            }
        };

        let msg = match Message::from_vec(&buf[..]) {
            Ok(m) if m.queries().len() > 0 => m,
            _ => {
                eprintln!("Could not deserialize received message");
                continue;
            }
        };

        let mut response = Message::new();
        response.set_id(msg.id());
        response.set_message_type(MessageType::Response);
        response.set_op_code(msg.op_code());
        response.set_recursion_desired(msg.recursion_desired());
        response.set_recursion_available(false);

        // TODO: check how to handle wildcards according to the relevant RFCs
        // (does a.b.example.com match *.example.com?)
        let q = &msg.queries()[0];
        let res = get_rrset(&mut con, q.name(), q.query_type());
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

        match socket.send_to(
            &response.to_vec().expect("Could not serialize response"),
            addr,
        ) {
            Ok(_) => (),
            Err(_) => eprintln!("Could not send response"),
        }
    }
}
