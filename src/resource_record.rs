use std::net::{Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub enum ResourceRecord {
    A(Ipv4Addr),
    A6(Ipv6Addr),
    Ns(String),
    Cname(String),
    Ptr(String),
    Soa { mname: String, rname: String, serial: u32, refresh: u32, retry: u32, expire: u32 },
    Mx { preference: u16, exchange: String },
    Txt(String),
    Dnskey { flags: u16, protocol: u8, algorithm: u8, key_data: Vec<u8> },
    Srv { priority: u16, weight: u16, port: u16, target: String },
    Caa { flags: u8, tag: String, value: String },
    Openpgpkey(Vec<u8>),
    Tlsa { usage: u8, selector: u8, matching_type: u8, data: Vec<u8> },
}