pub mod doh;
pub mod persistence;

use pektin_common::deadpool_redis::Pool;
use pektin_common::proto::op::{Edns, Message, MessageType, ResponseCode};
use pektin_common::proto::rr::{Name, RData, Record, RecordType};
use pektin_common::{get_authoritative_zones, DbEntry, RrSet};
use persistence::{get_rrset, get_rrsig, QueryResponse};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PektinError {
    #[error("{0}")]
    CommonError(#[from] pektin_common::PektinCommonError),
    #[error("db error")]
    DbError(#[from] pektin_common::deadpool_redis::redis::RedisError),
    #[error("could not create db connection pool: `{0}`")]
    PoolError(#[from] pektin_common::deadpool_redis::CreatePoolError),
    #[error("io error: `{0}`")]
    IoError(#[from] std::io::Error),
    #[error("could not (de)serialize JSON: `{0}`")]
    JsonError(#[from] serde_json::Error),
    #[error("invalid DNS data")]
    ProtoError(#[from] pektin_common::proto::error::ProtoError),
    #[error("data in db invalid")]
    InvalidDbData,
    #[error("requested db key had an unexpected type")]
    WickedDbValue,
}
pub type PektinResult<T> = Result<T, PektinError>;

pub async fn process_request(mut message: Message, db_pool: Pool, db_pool_dnssec: Pool) -> Message {
    // TODO: better error logging (use https://git.sr.ht/~mvforell/ddc-ci/tree/master/item/src/daemon_data.rs#L90 ?)

    let mut response = Message::new();
    response.set_id(message.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(message.op_code());
    response.set_recursion_desired(message.recursion_desired());
    response.set_recursion_available(false);
    response.set_authoritative(true);

    let mut edns = Edns::new();
    edns.set_max_payload(4096);
    response.set_edns(edns);

    let mut con = match db_pool.get().await {
        Ok(c) => c,
        Err(_) => {
            response.set_response_code(ResponseCode::ServFail);
            eprintln!("ServFail: could not get db connection from pool");

            // copy queries
            response.add_queries(message.take_queries().into_iter());
            return response;
        }
    };

    // TODO: check how to handle wildcards according to the relevant RFCs
    // (does a.b.example.com match *.example.com?)

    // needed later for querying the SOA record if no query could be answered
    let mut zone_name = None;
    // keep track if we added an answer into the message (response.answer_count() doesn't automatically update)
    let mut answer_stored = false;

    for q in message.queries() {
        if zone_name.is_none() {
            zone_name = Some(q.name());
        }

        let res = get_rrset(&mut con, q.name(), q.query_type()).await;
        if let Ok(db_response) = res {
            let db_entry = match db_response {
                QueryResponse::Empty => continue,
                QueryResponse::Definitive(def) => def,
                QueryResponse::Wildcard(wild) => wild,
                QueryResponse::Both { definitive, .. } => definitive,
            };
            let records = match db_entry.convert() {
                Ok(r) => r,
                Err(err) => {
                    eprintln!("{}", err);
                    response.set_response_code(ResponseCode::ServFail);
                    break;
                }
            };
            response.add_answers(records);
            answer_stored = true;
        } else {
            eprintln!("{}", res.err().unwrap());
            response.set_response_code(ResponseCode::ServFail);
            break;
        }

        let do_flag = message.edns().map(|edns| edns.dnssec_ok()).unwrap_or(false);
        if answer_stored && do_flag {
            let mut dnssec_con = match db_pool_dnssec.get().await {
                Ok(c) => c,
                Err(_) => {
                    response.set_response_code(ResponseCode::ServFail);
                    eprintln!("ServFail: could not get db dnssec connection from pool");

                    // copy queries
                    response.add_queries(message.take_queries().into_iter());
                    return response;
                }
            };

            let res = get_rrsig(&mut dnssec_con, q.name(), q.query_type()).await;
            if let Ok(db_response) = res {
                let db_entry = match db_response {
                    QueryResponse::Empty => continue,
                    QueryResponse::Definitive(def) => def,
                    QueryResponse::Wildcard(wild) => wild,
                    QueryResponse::Both { definitive, .. } => definitive,
                };
                let records = match db_entry.convert() {
                    Ok(r) => r,
                    Err(err) => {
                        eprintln!("{}", err);
                        response.set_response_code(ResponseCode::ServFail);
                        break;
                    }
                };
                response.add_answers(records);
            } else {
                eprintln!("{}", res.err().unwrap());
                response.set_response_code(ResponseCode::ServFail);
                break;
            }
        }
    }

    if !answer_stored && (response.response_code() != ResponseCode::ServFail) {
        if let Some(queried_name) = zone_name {
            match get_authoritative_zones(&mut con).await {
                Ok(authoritative_zones) => {
                    fn find_authoritative_zone(
                        name: &Name,
                        authoritative_zones: &[Name],
                    ) -> Option<Name> {
                        let mut authoritative_zones = authoritative_zones.to_owned();
                        // the - makes it sort the zones with the most labels first
                        authoritative_zones.sort_by_key(|zone| -(zone.num_labels() as i16));
                        authoritative_zones
                            .into_iter()
                            .find(|zone| zone.zone_of(name))
                    }

                    let authoritative_zones: Vec<_> = authoritative_zones
                        .into_iter()
                        .map(|zone| {
                            Name::from_utf8(zone).expect("Name in db is not a valid DNS name")
                        })
                        .collect();
                    if let Some(auth_zone) =
                        find_authoritative_zone(queried_name, &authoritative_zones)
                    {
                        let res = get_rrset(&mut con, &auth_zone, RecordType::SOA).await;
                        match res {
                            Ok(db_response) => {
                                let db_entry = match db_response {
                                    QueryResponse::Empty => {
                                        response.set_response_code(ResponseCode::Refused);
                                        None
                                    }
                                    QueryResponse::Definitive(def) => Some(def),
                                    QueryResponse::Wildcard(wild) => Some(wild),
                                    QueryResponse::Both { definitive, .. } => Some(definitive),
                                };
                                let (ttl, rr_set) = match db_entry {
                                    Some(DbEntry {
                                        rr_set: RrSet::SOA { rr_set },
                                        ttl,
                                        ..
                                    }) => (ttl, rr_set),
                                    _ => (0, vec![]),
                                };
                                for record in rr_set {
                                    // get the name of the authoritative zone, preserving the case of the queried name
                                    let mut soa_name = queried_name.clone();
                                    while soa_name.num_labels() != auth_zone.num_labels() {
                                        soa_name = soa_name.base_name();
                                    }
                                    let rr =
                                        Record::from_rdata(soa_name, ttl, RData::SOA(record.value));
                                    // the name is a bit misleading; this adds the record to the authority section
                                    response.add_name_server(rr);
                                    // TODO this is probably not correct (see RFC)
                                    response.set_response_code(ResponseCode::NXDomain);
                                }
                            }
                            Err(e) => {
                                eprintln!("Could not get SOA record: {}", e);
                                response.set_response_code(ResponseCode::ServFail);
                            }
                        }
                    } else {
                        response.set_response_code(ResponseCode::Refused);
                    }
                }
                Err(e) => {
                    eprintln!("Could not get authoritative zones: {}", e);
                    response.set_response_code(ResponseCode::ServFail);
                }
            }
        } else {
            // TODO is that right?
            response.set_response_code(ResponseCode::Refused);
        }
    }

    // copy queries
    response.add_queries(message.take_queries().into_iter());

    response
}
