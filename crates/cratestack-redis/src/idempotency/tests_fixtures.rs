#![cfg(test)]

use redis::Value as RedisValue;

use super::store::RedisIdempotencyStore;

pub(super) fn offline_store(prefix: &str) -> RedisIdempotencyStore {
    let client = redis::Client::open("redis://127.0.0.1/").expect("static URL must parse offline");
    RedisIdempotencyStore::from_client(client, prefix)
}

pub(super) fn bulk(s: &str) -> RedisValue {
    RedisValue::BulkString(s.as_bytes().to_vec())
}

pub(super) fn raw_bulk(b: impl AsRef<[u8]>) -> RedisValue {
    RedisValue::BulkString(b.as_ref().to_vec())
}
