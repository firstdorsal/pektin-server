use dotenv::dotenv;
use pektin::load_env;
use pektin::persistence::get_rrset;
use redis::Client;
use trust_dns_proto::rr::Record;
use std::error::Error;
use std::net::UdpSocket;
use trust_dns_proto::op::{Edns, Message, MessageType, ResponseCode};

const D_BIND_ADDRESS: &'static str = "0.0.0.0";
const D_BIND_PORT: &'static str = "53";
const D_REDIS_URI: &'static str = "redis://redis:6379";

const RECV_BUFSIZE: usize = 4096;

fn main() -> Result<(), Box<dyn Error>> {
    dotenv().ok();

    println!("Started Pektin with these globals:");
    let redis_uri = load_env(D_REDIS_URI, "REDIS_URI");
    let bind_address = load_env(D_BIND_ADDRESS, "BIND_ADDRESS"); 
    let bind_port = load_env(D_BIND_PORT, "BIND_PORT");

    let client = Client::open(redis_uri)?;
    let mut con = client.get_connection()?;

    let socket = UdpSocket::bind(format!("{}:{}", bind_address, bind_port))?;
    let mut buf = [0; RECV_BUFSIZE];
    let (_, addr) = socket.recv_from(&mut buf)?;

    let msg = Message::from_vec(&buf[..])?;

    let mut response = Message::new();
    response.set_id(msg.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(msg.op_code());
    response.set_recursion_desired(msg.recursion_desired());
    response.set_recursion_available(false);

    let q = &msg.queries()[0];
    let res = get_rrset(&mut con, q.name(), q.query_type());
    if let Ok(rrset) = res {
        for record in rrset.rr_set {
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

    socket.send_to(&response.to_vec().unwrap(), addr).expect("could not send response back to client");

    Ok(())
}
