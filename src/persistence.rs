use crate::PektinResult;
use redis::{Commands, Connection};
use serde::{Deserialize, Serialize};
use trust_dns_proto::rr::{Name, RData, RecordType};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ResourceRecord {
    pub ttl: u32,
    pub value: RData,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RrSet {
    pub rr_type: RecordType,
    pub rr_set: Vec<ResourceRecord>,
}

pub fn get_rrset(con: &mut Connection, zone: &Name, rr_type: RecordType) -> PektinResult<RrSet> {
    let key = format!("{}:{}", zone, rr_type);
    let rrset_json: String = con.get(key)?;
    Ok(serde_json::from_str(&rrset_json)?)
}