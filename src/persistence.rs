use crate::{PektinError, PektinResult};
use pektin_common::deadpool_redis::redis::aio::Connection;
use pektin_common::deadpool_redis::redis::{AsyncCommands, FromRedisValue, Value};
use pektin_common::proto::rr::{Name, RecordType};
use pektin_common::RedisEntry;

pub enum QueryResponse {
    Empty,
    Definitive(RedisEntry),
    Wildcard(RedisEntry),
    Both {
        definitive: RedisEntry,
        wildcard: RedisEntry,
    },
}

// also automatically looks for a wildcard record
pub async fn get_rrset(
    con: &mut Connection,
    zone: &Name,
    rr_type: RecordType,
) -> PektinResult<QueryResponse> {
    let definitive_key = format!("{}:{}", zone, rr_type);
    let wildcard_key = format!("{}:{}", zone.clone().into_wildcard(), rr_type);
    let res: Vec<Value> = con.get(vec![definitive_key, wildcard_key]).await?;
    if res.len() != 2 {
        return Err(PektinError::InvalidRedisData);
    }

    let string_res = (
        String::from_redis_value(&res[0]),
        String::from_redis_value(&res[1]),
    );

    Ok(match string_res {
        (Ok(def), Ok(wild)) => QueryResponse::Both {
            definitive: serde_json::from_str(&def)?,
            wildcard: serde_json::from_str(&wild)?,
        },
        (Ok(def), Err(_)) => {
            if !matches!(res[1], Value::Nil) {
                return Err(PektinError::WickedRedisValue);
            }
            QueryResponse::Definitive(serde_json::from_str(&def)?)
        }
        (Err(_), Ok(wild)) => {
            if !matches!(res[0], Value::Nil) {
                return Err(PektinError::WickedRedisValue);
            }
            QueryResponse::Wildcard(serde_json::from_str(&wild)?)
        }
        (Err(_), Err(_)) => {
            if !matches!(res[0], Value::Nil) || !matches!(res[1], Value::Nil) {
                return Err(PektinError::WickedRedisValue);
            }
            QueryResponse::Empty
        }
    })
}
