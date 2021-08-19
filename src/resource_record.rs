use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, Ipv6Addr};
use trust_dns_proto::rr::dnssec::Algorithm;
use trust_dns_proto::rr::dnssec::rdata::DNSKEY;
use trust_dns_proto::rr::rdata::tlsa::{CertUsage, Matching, Selector};
use trust_dns_proto::rr::rdata::{CAA, MX, OPENPGPKEY, SOA, SRV, TLSA, TXT};
use trust_dns_proto::rr::{Name, RData};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub enum ResourceRecord {
    A(Ipv4Addr),
    AAAA(Ipv6Addr),
    NS(String),
    CNAME(String),
    PTR(String),
    SOA { mname: String, rname: String, serial: u32, refresh: i32, retry: i32, expire: i32, minimum: u32 },
    MX { preference: u16, exchange: String },
    TXT(String),
    DNSKEY { flags: u16, protocol: u8, algorithm: u8, key_data: Vec<u8> },
    SRV { priority: u16, weight: u16, port: u16, target: String },
    CAA { flags: u8, tag: String, value: String },
    OPENPGPKEY(Vec<u8>),
    TLSA { usage: u8, selector: u8, matching_type: u8, data: Vec<u8> },
}

impl From<ResourceRecord> for RData {
    fn from(value: ResourceRecord) -> RData {
        match value {
            ResourceRecord::A(addr) => {
                RData::A(addr)
            },
            ResourceRecord::AAAA(addr) => {
                RData::AAAA(addr)
            },
            ResourceRecord::NS(name) => {
                RData::NS(Name::from_ascii(name).expect("invalid name value for NS ResourceRecord: must be ASCII"))
            },
            ResourceRecord::CNAME(name) => {
                RData::CNAME(Name::from_ascii(name).expect("invalid name value for CNAME ResourceRecord: must be ASCII"))
            },
            ResourceRecord::PTR(name) => {
                RData::PTR(Name::from_ascii(name).expect("invalid name value for PTR ResourceRecord: must be ASCII"))
            },
            ResourceRecord::SOA { mname, rname, serial, refresh, retry, expire, minimum } => {
                let mname = Name::from_ascii(mname).expect("invalid mname value for SOA ResourceRecord: must be ASCII");
                let rname = Name::from_ascii(rname).expect("invalid rname value for SOA ResourceRecord: must be ASCII");
                RData::SOA(SOA::new(mname, rname, serial, refresh, retry, expire, minimum))
            },
            ResourceRecord::MX { preference, exchange } => {
                let exchange = Name::from_ascii(exchange).expect("invalid exchange value for MX ResourceRecord: must be ASCII");
                RData::MX(MX::new(preference, exchange))
            },
            ResourceRecord::TXT(text) => {
                RData::TXT(TXT::new(vec![text]))
            },
            ResourceRecord::DNSKEY { flags, protocol, algorithm, key_data } => {
                let zone_key = (flags & 0b00000001_00000000) != 0;
                let secure_entry_point = (flags & 0b00000000_00000001) != 0;
                let revoke = (flags & 0b00000000_10000000) != 0;
                assert_eq!(protocol, 3, "invalid protocol value for DNSKEY ResourceRecord: must be 3 according to RFC 4043");
                DNSKEY::new(
                    zone_key, secure_entry_point, revoke, Algorithm::from_u8(algorithm), key_data
                ).into()
            },
            ResourceRecord::SRV { priority, weight, port, target } => {
                let target = Name::from_ascii(target).expect("invalid target value for SRV ResourceRecord: must be ASCII");
                RData::SRV(SRV::new(priority, weight, port, target))
            },
            ResourceRecord::CAA { flags, tag, value } => {
                let issuer_critical = (flags & 0b10000000) != 0;
                // TODO: refactor ResourceRecord::CAA to (better) match the trust_dns_proto equivalent,
                // i.e. don't allow invalid tags and embed the according values into the struct
                // (probably needs its own enum)
                RData::CAA(match tag.as_str() {
                    "issue" => CAA::new_issue(issuer_critical, todo!(), todo!()),
                    "issuewild" => CAA::new_issuewild(issuer_critical, todo!(), todo!()),
                    "iodef" => CAA::new_iodef(issuer_critical, todo!()),
                    _ => panic!("invalid tag value for CAA ResourceRecord: must be issue, issuewild or iodef"),
                })
            },
            ResourceRecord::OPENPGPKEY(key) => {
                RData::OPENPGPKEY(OPENPGPKEY::new(key))
            },
            ResourceRecord::TLSA { usage, selector, matching_type, data } => {
                RData::TLSA(TLSA::new(
                    CertUsage::from(usage), Selector::from(selector), Matching::from(matching_type), data
                ))
            },
        }
    }
}