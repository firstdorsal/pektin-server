use dotenv::dotenv;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use pektin::resource_record::ResourceRecord;
use redis::Client;
use pektin::{PektinResult, load_env};
use pektin::persistence::{set_list, get_list, delete_list};

const D_REDIS_URI: &'static str = "redis://redis:6379";

fn main() -> PektinResult<()> {
    dotenv().ok();

    let redis_uri = load_env(D_REDIS_URI, "REDIS_URI");
    let client = Client::open(redis_uri)?;
    let mut con = client.get_connection()?;

    let ttl = 300;
    let a = serde_json::to_string(&ResourceRecord::A(Ipv4Addr::from_str("2.56.96.115").unwrap())).unwrap();
    let aaaa = serde_json::to_string(&ResourceRecord::AAAA(Ipv6Addr::from_str("2a03:4000:3e:dd::1").unwrap())).unwrap();
    let ns1 = serde_json::to_string(&ResourceRecord::NS("e-flat.vonforell.de.".into())).unwrap();
    let ns2 = serde_json::to_string(&ResourceRecord::NS("g-flat.vonforell.de.".into())).unwrap();
    let ns3 = serde_json::to_string(&ResourceRecord::NS("b-flat.vonforell.de.".into())).unwrap();

    set_list(&mut con, "vonforell.de.:A", &[format!("{} {}", ttl, a)], true)?;
    set_list(&mut con, "vonforell.de.:AAAA", &[format!("{} {}", ttl, aaaa)], true)?;
    set_list(&mut con, "vonforell.de.:NS", &[format!("{} {}", ttl, ns1), format!("{} {}", ttl, ns2), format!("{} {}", ttl, ns3)], true)?;

    let a: Vec<String> = get_list(&mut con, "vonforell.de.:A")?;
    let aaaa: Vec<String> = get_list(&mut con, "vonforell.de.:AAAA")?;
    let ns: Vec<String> = get_list(&mut con, "vonforell.de.:NS")?;

    println!("{:?}", a);
    println!("{:?}", aaaa);
    println!("{:?}", ns);

    // delete_list(&mut con, "vonforell.de.:A")?;
    // delete_list(&mut con, "vonforell.de.:AAAA")?;
    // delete_list(&mut con, "vonforell.de.:NS")?;

    Ok(())
}
