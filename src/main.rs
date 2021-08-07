use pektin::persistence::{REDIS_URL, get_list};
use redis::Client;
use std::error::Error;
use std::net::{Ipv4Addr, UdpSocket};
use std::str::FromStr;
use trust_dns_proto::op::{Edns, Message, MessageType, ResponseCode};
use trust_dns_proto::rr::{RData, Record};

const BIND_ADDR: &'static str = "0.0.0.0";
const BIND_PORT: u16 = 5354;
const RECV_BUFSIZE: usize = 4096;

fn main() -> Result<(), Box<dyn Error>> {
    let client = Client::open(REDIS_URL)?;
    let mut con = client.get_connection()?;

    let socket = UdpSocket::bind(format!("{}:{}", BIND_ADDR, BIND_PORT))?;
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
    let key = format!("{}:{}", q.name().to_ascii(), q.query_type().to_string());
    let res = get_list::<_, String>(&mut con, key);
    if let Ok(entries) = res {
        for e in entries {
            let addr = Ipv4Addr::from_str(&e)?;
            response.add_answer(Record::from_rdata(q.name().clone(), 300, RData::A(addr)));
        }

        let mut edns = Edns::new();
        edns.set_max_payload(4096);
        response.set_edns(edns);
    } else {
        response.set_response_code(ResponseCode::ServFail);
    }

    socket.send_to(&response.to_vec().unwrap(), addr).expect("could not send response back to client");

    Ok(())
}
