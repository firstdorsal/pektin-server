use redis::{Client, RedisResult};
use pektin::persistence::{REDIS_URL, set_list, get_list, delete_list};

fn main() -> RedisResult<()> {
    let client = Client::open(REDIS_URL)?;
    let mut con = client.get_connection()?;
    // set_list(&mut con, "vonforell.de.:A", &["2.56.96.115"])?;
    // set_list(&mut con, "vonforell.de.:AAAA", &["2a03:4000:3e:dd::1"])?;
    // set_list(&mut con, "vonforell.de.:NS", &["e-flat.vonforell.de.", "g-flat.vonforell.de.", "b-flat.vonforell.de."])?;

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