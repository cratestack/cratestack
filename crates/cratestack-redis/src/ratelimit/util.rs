use cratestack_core::CratestackError;

use sha2::{Digest, Sha256};

pub(super) fn nibble_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + nibble - 10) as char,
        _ => unreachable!("nibble must be 0..=15"),
    }
}

/// Lowercase sha256 hex of a caller-supplied key.
///
/// Hashing keeps Redis keys a fixed length and sidesteps escaping around
/// `:` in user-supplied values — same shape as the idempotency store. It
/// is NOT a privacy measure: the input is already a hash or a peer address.
pub(super) fn key_hash(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(nibble_hex(byte >> 4));
        out.push(nibble_hex(byte & 0x0f));
    }
    out
}

/// How much of a bucket hash goes into a scope's member set.
///
/// 16 hex chars is 64 bits. Collisions inside ONE scope's set of at most a
/// few thousand members are ~2^-40 by the birthday bound, and a collision
/// costs an attacker nothing they could not get by simply not rotating the
/// header — it lets one bucket look already-admitted. In exchange the set
/// is a quarter the size, which matters because the set is the memory this
/// whole mechanism adds.
pub(super) const SCOPE_MEMBER_HEX_LEN: usize = 16;

/// The member string for `key` inside a scope set.
pub(super) fn scope_member(key: &str) -> String {
    let mut hash = key_hash(key);
    hash.truncate(SCOPE_MEMBER_HEX_LEN);
    hash
}

/// Public message for a transport-class failure. Fixed text, never the
/// driver's: [`CratestackError::Unavailable`] is a 4xx-shaped variant whose
/// payload IS the caller-visible message (see `cratestack-core/src/error.rs`),
/// and `redis::RedisError`'s `Display` routinely names the host and port it
/// failed to reach. The operator-side detail is not lost — the retry path
/// logs the full error before this conversion ever happens (`super::retry`).
pub(super) const STORE_UNAVAILABLE_MESSAGE: &str = "rate limit store temporarily unavailable";

/// Classify a Redis failure into the two categories the rate-limit layer
/// makes a *policy* decision on (cratestack#846 security review):
///
/// - **Transport-class** (`Unavailable`, 503): the connection broke or is
///   unusable. Nothing the caller sent caused it, no caller can fix it,
///   and it self-heals once the socket is replaced. This is the only
///   class `StoreErrorPolicy::Allow` will serve through.
/// - **Everything else** (`Internal`, 500): the server was reached and
///   answered with a refusal — `OOM command not allowed when used memory`,
///   a `NOPERM`, a malformed reply. These do not self-heal, and — the
///   finding that forced this split — an unauthenticated caller CAN cause
///   one: the default key function hashes an unvalidated `Authorization`
///   header, so rotating that header mints one Redis key per request
///   until the instance reaches `maxmemory`. Flattening that into the
///   same error as a broken pipe is what made fail-open reachable as a
///   global limiter bypass.
///
/// The predicate is `is_unrecoverable_error()` — precisely the set
/// `redis::aio::ConnectionManager` itself reconnects on
/// (`RetryMethod::Reconnect`/`ReconnectFromInitialConnections`), so "the
/// driver considers this connection finished" and "we treat it as
/// transport-class" cannot drift apart. It covers `ErrorKind::Parse` (a
/// half-read reply from a dying socket), which `is_connection_dropped()`
/// misses.
///
/// It also covers `AuthenticationFailed`, which is a deployment
/// misconfiguration rather than a transport fault. Kept in deliberately:
/// the test that matters for this predicate is *not caller-inducible*,
/// and a wrong Redis password is not something a request can provoke —
/// so treating it as fail-open does not hand anyone a bypass, and the
/// alternative (refusing every rate-limited route because a credential
/// rotated) is the outage this policy exists to avoid. It is not silent:
/// the per-10s `WARN` in `super::retry` carries the driver's message, so
/// a misconfigured deployment says `AuthenticationFailed` in the log from
/// the first request onward. Timeouts are excluded
/// (`RetryMethod::RetryImmediately`) — see `super::retry` for why
/// re-issuing a non-idempotent consume against a merely-slow server is
/// the wrong move.
pub(super) fn is_transport_class(error: &redis::RedisError) -> bool {
    error.is_unrecoverable_error()
}

pub(super) fn redis_error(error: redis::RedisError) -> CratestackError {
    if is_transport_class(&error) {
        CratestackError::Unavailable(STORE_UNAVAILABLE_MESSAGE.to_owned())
    } else {
        CratestackError::Internal(format!("redis rate limit: {error}"))
    }
}
