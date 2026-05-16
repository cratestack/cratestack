use cratestack_core::CoolError;

pub(super) fn nibble_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + nibble - 10) as char,
        _ => unreachable!("nibble must be 0..=15"),
    }
}

pub(super) fn redis_error(error: redis::RedisError) -> CoolError {
    CoolError::Internal(format!("redis idempotency: {error}"))
}
