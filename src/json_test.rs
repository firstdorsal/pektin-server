use pektin_common::{RedisValue, ResourceRecord};
use pektin_server::PektinResult;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use trust_dns_proto::rr::dnssec::rdata::DNSSECRData;
use trust_dns_proto::rr::dnssec::rdata::DNSKEY;
use trust_dns_proto::rr::dnssec::Algorithm;
use trust_dns_proto::rr::rdata::tlsa::CertUsage;
use trust_dns_proto::rr::rdata::tlsa::Matching;
use trust_dns_proto::rr::rdata::tlsa::Selector;
use trust_dns_proto::rr::rdata::TLSA;
use trust_dns_proto::rr::rdata::{CAA, MX, OPENPGPKEY, SOA, SRV, TXT};
use trust_dns_proto::rr::RecordType;
use trust_dns_proto::rr::{Name, RData};
use url::Url;

fn main() -> PektinResult<()> {
    let name = Name::from_str("vonforell.de.")?;

    let a = RData::A(Ipv4Addr::from_str("2.56.96.115").unwrap());
    let aaaa = RData::AAAA(Ipv6Addr::from_str("2a03:4000:3e:dd::1").unwrap());
    let caa = RData::CAA(CAA::new_iodef(
        true,
        Url::from_str("http://example.com").unwrap(),
    ));
    let ns = RData::NS(name.clone());
    let cname = RData::CNAME(name.clone());
    let ptr = RData::PTR(name.clone());
    let soa = RData::SOA(SOA::new(
        name.clone(),
        name.clone(),
        202108101,
        3600,
        7200,
        14400,
        3600,
    ));
    let mx = RData::MX(MX::new(10, name.clone()));
    let txt = RData::TXT(TXT::new(vec!["string".into()]));
    let pgp = RData::OPENPGPKEY(OPENPGPKEY::new(vec![0, 1, 2, 3]));
    let srv = RData::SRV(SRV::new(10, 10, 10, name));
    let dnskey = RData::DNSSEC(DNSSECRData::DNSKEY(DNSKEY::new(
        true,
        true,
        false,
        Algorithm::ED25519,
        vec![0, 1, 2, 3],
    )));
    let tlsa = RData::TLSA(TLSA::new(
        CertUsage::CA,
        Selector::Full,
        Matching::Sha256,
        vec![0, 1, 2, 3],
    ));

    println!("{}", serde_json::to_string(&a)?);
    println!("{}", serde_json::to_string(&aaaa)?);
    println!("{}", serde_json::to_string(&caa)?);
    println!("{}", serde_json::to_string(&ns)?);
    println!("{}", serde_json::to_string(&cname)?);
    println!("{}", serde_json::to_string(&ptr)?);
    println!("{}", serde_json::to_string(&soa)?);
    println!("{}", serde_json::to_string(&mx)?);
    println!("{}", serde_json::to_string(&txt)?);
    println!("{}", serde_json::to_string(&pgp)?);
    println!("{}", serde_json::to_string(&srv)?);
    println!("{}", serde_json::to_string(&dnskey)?);
    println!("{}", serde_json::to_string(&tlsa)?);

    let rrset = RedisValue {
        rr_type: RecordType::A,
        rr_set: vec![
            ResourceRecord {
                ttl: 300,
                value: a.clone(),
            },
            ResourceRecord {
                ttl: 3600,
                value: a,
            },
        ],
    };
    println!("{}", serde_json::to_string(&rrset)?);

    Ok(())
}
