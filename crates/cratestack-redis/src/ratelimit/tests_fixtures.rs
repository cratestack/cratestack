#![cfg(test)]

use redis::Value as RedisValue;

use super::store::RedisRateLimitStore;

pub(super) fn offline_store(prefix: &str) -> RedisRateLimitStore {
    let client =
        redis::Client::open("redis://127.0.0.1/").expect("static URL must parse offline");
    RedisRateLimitStore::from_client(client, prefix)
}

pub(super) fn bulk(s: &str) -> RedisValue {
    RedisValue::BulkString(s.as_bytes().to_vec())
}

pub(super) fn test_seed() -> u64 {
    std::env::var("CRATESTACK_TEST_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

pub(super) struct XorShift64(u64);

impl XorShift64 {
    pub(super) fn new(seed: u64) -> Self {
        // Avoid the all-zero state which would lock the PRNG.
        Self(if seed == 0 { 0xDEAD_BEEF_CAFE_BABE } else { seed })
    }
    pub(super) fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub(super) fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }
    pub(super) fn next_range(&mut self, lo: u32, hi: u32) -> u32 {
        debug_assert!(lo <= hi);
        lo + (self.next_u32() % (hi - lo + 1))
    }
    pub(super) fn next_bytes(&mut self, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        while out.len() < len {
            out.extend_from_slice(&self.next_u64().to_le_bytes());
        }
        out.truncate(len);
        out
    }
    pub(super) fn next_string(&mut self, max_len: usize) -> String {
        let len = (self.next_u32() as usize) % (max_len + 1);
        // Include `:` and NUL routinely — they're the bytes most
        // likely to break key-derivation logic naïvely.
        const ALPHABET: &[u8] = b"abcdefghij0123456789:\0 -_";
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            let idx = (self.next_u32() as usize) % ALPHABET.len();
            s.push(ALPHABET[idx] as char);
        }
        s
    }
}
