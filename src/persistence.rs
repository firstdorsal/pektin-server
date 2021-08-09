use crate::{PektinError, PektinResult, resource_record::ResourceRecord};
use redis::{Connection, FromRedisValue, ToRedisArgs};
use trust_dns_proto::rr::{Name, Record, RecordType};

pub fn get_rrs(con: &mut Connection, zone: &Name, rr_type: RecordType) -> PektinResult<Vec<Record>>
{
    let mut rrs = vec![];
    let key = format!("{}:{}", zone, rr_type);
    let entries = get_list::<_, String>(con, key)?;
    for entry in &entries {
        if let Some((ttl, json)) = entry.split_once(' ') {
            let rr: ResourceRecord = serde_json::from_str(json)?;
            let ttl = ttl.parse::<u32>().map_err(|_| PektinError::InvalidRedisData)?;
            rrs.push(Record::from_rdata(zone.clone(), ttl, rr.into()));
        } else {
            return Err(PektinError::InvalidRedisData);
        }
    }
    Ok(rrs)
}

pub fn set_list<K, T, V>(con: &mut Connection, key: K, values: V, clear: bool) -> PektinResult<()>
where
    K: AsRef<str>,
    V: AsRef<[T]>,
    T: ToRedisArgs,
{
    let key = key.as_ref();
    if clear {
        delete_list(con, key)?;
    }
    let mut cmd = redis::cmd("RPUSH");
    cmd.arg(key);
    for val in values.as_ref() {
        cmd.arg(val);
    }
    Ok(cmd.query(con)?)
}

pub fn get_list<K, V>(con: &mut Connection, key: K) -> PektinResult<Vec<V>>
where
    K: AsRef<str>,
    V: FromRedisValue,
{
    Ok(redis::cmd("LRANGE").arg(key.as_ref()).arg(0).arg(-1).query(con)?)
}

pub fn delete_list<K>(con: &mut Connection, key: K) -> PektinResult<()>
where
    K: AsRef<str>,
{
    Ok(redis::cmd("DEL").arg(key.as_ref()).query(con)?)
}