pub mod resource_record;

pub mod persistence {
    use redis::{Connection, FromRedisValue, RedisResult, ToRedisArgs};

    pub const REDIS_URL: &'static str = "redis://127.0.0.1:6379";

    pub fn set_list<K, T, V>(con: &mut Connection, key: K, values: V) -> RedisResult<()>
    where
        K: AsRef<str>,
        V: AsRef<[T]>,
        T: ToRedisArgs,
    {
        let mut cmd = redis::cmd("RPUSH");
        cmd.arg(key.as_ref());
        for val in values.as_ref() {
            cmd.arg(val);
        }
        cmd.query(con)
    }

    pub fn get_list<K, V>(con: &mut Connection, key: K) -> RedisResult<Vec<V>>
    where
        K: AsRef<str>,
        V: FromRedisValue,
    {
        Ok(redis::cmd("LRANGE").arg(key.as_ref()).arg(0).arg(-1).query(con)?)
    }

    pub fn delete_list<K>(con: &mut Connection, key: K) -> RedisResult<()>
    where
        K: AsRef<str>,
    {
        redis::cmd("DEL").arg(key.as_ref()).query(con)
    }
}