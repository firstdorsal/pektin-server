pub mod doh;
pub mod persistence;

use pektin_common::deadpool_redis::Pool;
use pektin_common::proto::op::{Edns, Message, MessageType, ResponseCode};
use pektin_common::proto::rr::{Name, Record, RecordType};
use pektin_common::{get_authoritative_zones, RedisEntry};
use persistence::{get_rrset, QueryResponse};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PektinError {
    #[error("{0}")]
    CommonError(#[from] pektin_common::PektinCommonError),
    #[error("redis error")]
    RedisError(#[from] pektin_common::deadpool_redis::redis::RedisError),
    #[error("could not create redis connection pool: `{0}`")]
    PoolError(#[from] pektin_common::deadpool_redis::CreatePoolError),
    #[error("io error: `{0}`")]
    IoError(#[from] std::io::Error),
    #[error("could not (de)serialize JSON: `{0}`")]
    JsonError(#[from] serde_json::Error),
    #[error("invalid DNS data")]
    ProtoError(#[from] pektin_common::proto::error::ProtoError),
    #[error("data in redis invalid")]
    InvalidRedisData,
    #[error("requested redis key had an unexpected type")]
    WickedRedisValue,
}
pub type PektinResult<T> = Result<T, PektinError>;

pub async fn process_request(mut message: Message, redis_pool: Pool) -> Message {
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

    let mut con = match redis_pool.get().await {
        Ok(c) => c,
        Err(_) => {
            response.set_response_code(ResponseCode::ServFail);

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
        if let Ok(redis_response) = res {
            let redis_entry = match redis_response {
                QueryResponse::Empty => continue,
                QueryResponse::Definitive(def) => def,
                QueryResponse::Wildcard(wild) => wild,
                QueryResponse::Both { definitive, .. } => definitive,
            };
            let records: Vec<Record> = match redis_entry.try_into() {
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
    }

    if !answer_stored && (response.response_code() != ResponseCode::ServFail) {
        if let Some(queried_name) = zone_name {
            if let Ok(authoritative_zones) = get_authoritative_zones(&mut con).await {
                if let Some(auth_zone) = authoritative_zones
                    .into_iter()
                    .map(|zone| {
                        Name::from_utf8(zone).expect("Name in redis is not a valid DNS name")
                    })
                    .find(|zone| zone.zone_of(queried_name))
                {
                    let res = get_rrset(&mut con, &auth_zone, RecordType::SOA).await;
                    if let Ok(redis_response) = res {
                        let redis_entry: Option<RedisEntry> = match redis_response {
                            QueryResponse::Empty => {
                                response.set_response_code(ResponseCode::Refused);
                                None
                            }
                            QueryResponse::Definitive(def) => Some(def),
                            QueryResponse::Wildcard(wild) => Some(wild),
                            QueryResponse::Both { definitive, .. } => Some(definitive),
                        };
                        if let Some(redis_entry) = redis_entry {
                            let records: Vec<Record> = match redis_entry.try_into() {
                                Ok(r) => r,
                                Err(err) => {
                                    eprintln!("{}", err);
                                    response.set_response_code(ResponseCode::ServFail);
                                    vec![]
                                }
                            };
                            for record in records {
                                // get the name of the authoritative zone, preserving the case of the queried name
                                let mut soa_name = queried_name.clone();
                                while soa_name.num_labels() != auth_zone.num_labels() {
                                    soa_name = soa_name.base_name();
                                }
                                let ttl = record.ttl();
                                let rr = Record::from_rdata(soa_name, ttl, record.into_data());
                                // the name is a bit misleading; this adds the record to the authority section
                                response.add_name_server(rr);
                                response.set_response_code(ResponseCode::NXDomain);
                            }
                        }
                    } else {
                        response.set_response_code(ResponseCode::ServFail);
                    }
                } else {
                    response.set_response_code(ResponseCode::Refused);
                }
            } else {
                response.set_response_code(ResponseCode::ServFail);
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
