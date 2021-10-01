use thiserror::Error;

pub mod persistence;
pub use trust_dns_proto as proto;

#[derive(Debug, Error)]
pub enum PektinError {
    #[error("{0}")]
    CommonError(#[from] pektin_common::PektinCommonError),
    #[error("redis error")]
    RedisError(#[from] deadpool_redis::redis::RedisError),
    #[error("could not create redis connection pool: `{0}`")]
    PoolError(#[from] deadpool_redis::CreatePoolError),
    #[error("io error: `{0}`")]
    IoError(#[from] std::io::Error),
    #[error("could not (de)serialize JSON: `{0}`")]
    JsonError(#[from] serde_json::Error),
    #[error("invalid DNS data")]
    ProtoError(#[from] trust_dns_proto::error::ProtoError),
    #[error("data in redis invalid")]
    InvalidRedisData,
    #[error("requested redis key had an unexpected type")]
    WickedRedisValue,
}
pub type PektinResult<T> = Result<T, PektinError>;
