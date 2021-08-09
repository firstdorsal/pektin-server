use std::env;

pub mod resource_record;

pub mod persistence {
    use redis::{Connection, FromRedisValue, RedisResult, ToRedisArgs};

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

pub fn load_env(default_parameter: &str, parameter_name: &str) -> String {
    let mut p = String::from(default_parameter);
    if let Ok(param) = env::var(parameter_name) {
        if param.len() > 0 {
            p = param;
        }
    };
    println!("{}: {}", parameter_name, p);
    return p;
}