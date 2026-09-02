use cratestack_core::CratestackError;

pub(super) fn nibble_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + nibble - 10) as char,
        _ => unreachable!("nibble must be 0..=15"),
    }
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
/// misses. It also covers `AuthenticationFailed`, which is a deployment
/// misconfiguration rather than a transport fault; included because the
/// driver classifies it as reconnect-worthy and because it is equally not
/// caller-reachable. Timeouts are excluded
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
