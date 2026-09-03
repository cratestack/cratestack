//! Which Redis failures are transport-class, and what `CratestackError`
//! each maps to (cratestack#846 security review).
//!
//! This classification is what gates the fail-open policy upstream in
//! `cratestack_axum::ratelimit`, so it is the security-relevant half of
//! this module: get it wrong in the permissive direction and an
//! unauthenticated caller who fills Redis switches the limiter off.
//!
//! Also holds the fixtures the retry tests share (`super::tests_retry`).

#![cfg(test)]

use std::io;
use std::time::Duration;

use cratestack_core::CratestackError;
use cratestack_core::log_throttle::LogThrottle;

use super::util::is_transport_class;

/// Zero interval: these tests are not about the log budget (that is
/// covered in `cratestack_core::log_throttle`), so never suppress.
pub(super) fn always_warn() -> LogThrottle {
    LogThrottle::new(Duration::from_secs(0))
}

pub(super) fn broken_pipe() -> redis::RedisError {
    io::Error::new(io::ErrorKind::BrokenPipe, "Broken pipe (os error 32)").into()
}

fn connection_reset() -> redis::RedisError {
    io::Error::new(io::ErrorKind::ConnectionReset, "Connection reset by peer").into()
}

/// A deterministic non-transport failure: the server answered, it just
/// answered with a refusal.
fn script_error() -> redis::RedisError {
    redis::RedisError::from((
        redis::ErrorKind::Server(redis::ServerErrorKind::NoScript),
        "No matching script",
    ))
}

/// The shape of the attack from the security review: a reachable Redis
/// that refuses because an unauthenticated caller filled it up. `OOM` is
/// not a known server-error code to the driver, so it arrives as an
/// extension error — deterministic, and emphatically not something a new
/// socket fixes.
pub(super) fn oom_error() -> redis::RedisError {
    redis::RedisError::from((
        redis::ErrorKind::Extension,
        "OOM command not allowed when used memory > 'maxmemory'",
    ))
}

#[test]
fn transport_class_covers_the_errors_a_fresh_socket_fixes() {
    assert!(is_transport_class(&broken_pipe()));
    assert!(is_transport_class(&connection_reset()));
}

#[test]
fn transport_class_excludes_deterministic_server_errors() {
    assert!(
        !is_transport_class(&script_error()),
        "retrying a deterministic server error just doubles the latency before the same failure"
    );
}

/// The classification that gates the fail-open policy upstream: an `OOM`
/// must NOT look like a broken pipe, or an unauthenticated caller who
/// fills Redis gets the limiter switched off globally.
#[test]
fn an_oom_is_not_transport_class_and_maps_to_internal() {
    assert!(!is_transport_class(&oom_error()));
    assert!(
        matches!(
            super::util::redis_error(oom_error()),
            CratestackError::Internal(_)
        ),
        "a reachable-but-refusing Redis must not be reported as Unavailable — that variant is \
         what `StoreErrorPolicy::Allow` serves through"
    );
}

/// The counterpart: a transport failure must map to `Unavailable`, and
/// must not leak the driver's message (which names host and port) into
/// what is, for that variant, the caller-visible string.
#[test]
fn a_broken_pipe_maps_to_unavailable_without_leaking_driver_detail() {
    let error = super::util::redis_error(broken_pipe());
    assert!(matches!(error, CratestackError::Unavailable(_)));
    let public = error.public_message().into_owned();
    assert_eq!(public, super::util::STORE_UNAVAILABLE_MESSAGE);
    assert!(
        !public.contains("Broken pipe") && !public.contains("os error"),
        "Unavailable's payload is the PUBLIC message; driver text must not ride along: {public}"
    );
}
