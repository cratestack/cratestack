//! cratestack#846: a stale pooled connection (Redis idle-timeout,
//! restart, network blip) must cost zero user-visible requests, not one.
//!
//! A real broken pipe cannot be induced deterministically against a
//! testcontainer — `CLIENT KILL` races the manager's background
//! reconnect, so the test would be flaky in exactly the way that teaches
//! people to ignore it. These drive the *shipped* retry function
//! (`super::retry::invoke_with_retry`, the same one `consume` calls)
//! with an injected connection that fails once, which is what the
//! production incident actually looked like from this code's side.

#![cfg(test)]

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cratestack_core::CratestackError;

use super::retry::{invoke_with_retry, is_connection_class};

fn broken_pipe() -> redis::RedisError {
    io::Error::new(io::ErrorKind::BrokenPipe, "Broken pipe (os error 32)").into()
}

fn connection_reset() -> redis::RedisError {
    io::Error::new(io::ErrorKind::ConnectionReset, "Connection reset by peer").into()
}

/// A deterministic non-connection failure: the server answered, it just
/// answered with an error.
fn script_error() -> redis::RedisError {
    redis::RedisError::from((
        redis::ErrorKind::Server(redis::ServerErrorKind::NoScript),
        "No matching script",
    ))
}

#[test]
fn connection_class_covers_the_errors_a_fresh_socket_fixes() {
    assert!(is_connection_class(&broken_pipe()));
    assert!(is_connection_class(&connection_reset()));
}

#[test]
fn connection_class_excludes_deterministic_server_errors() {
    assert!(
        !is_connection_class(&script_error()),
        "retrying a deterministic server error just doubles the latency before the same failure"
    );
}

#[tokio::test]
async fn a_broken_pipe_is_absorbed_by_exactly_one_retry() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let connects = Arc::new(AtomicUsize::new(0));

    let attempts_probe = attempts.clone();
    let connects_probe = connects.clone();
    let result: Result<&str, CratestackError> = invoke_with_retry(
        || {
            let connects = connects_probe.clone();
            async move {
                connects.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        },
        |()| {
            let attempts = attempts_probe.clone();
            async move {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(broken_pipe())
                } else {
                    Ok("allowed")
                }
            }
        },
    )
    .await;

    assert_eq!(
        result.expect("the retry must absorb a single broken pipe"),
        "allowed"
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 2, "exactly one retry");
    assert_eq!(
        connects.load(Ordering::SeqCst),
        2,
        "the retry must re-acquire the connection, not reuse the handle that just died"
    );
}

#[tokio::test]
async fn a_second_broken_pipe_is_surfaced_rather_than_retried_again() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_probe = attempts.clone();

    let result: Result<&str, CratestackError> = invoke_with_retry(
        || async { Ok(()) },
        |()| {
            let attempts = attempts_probe.clone();
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(broken_pipe())
            }
        },
    )
    .await;

    let error = result.expect_err("a store that is genuinely down must still fail");
    assert!(
        error.to_string().contains("redis rate limit"),
        "the surfaced error keeps this store's prefix: {error}"
    );
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        2,
        "the budget is exactly one retry — a loop would amplify load on an already-struggling \
         Redis"
    );
}

#[tokio::test]
async fn a_deterministic_error_is_not_retried_at_all() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_probe = attempts.clone();

    let result: Result<&str, CratestackError> = invoke_with_retry(
        || async { Ok(()) },
        |()| {
            let attempts = attempts_probe.clone();
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(script_error())
            }
        },
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "a NOSCRIPT/wrong-type reply is deterministic; retrying it only doubles the latency"
    );
}

/// The happy path must not pay for the retry machinery: one connect, one
/// attempt, no second round-trip.
#[tokio::test]
async fn a_successful_call_runs_exactly_once() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_probe = attempts.clone();

    let result: Result<&str, CratestackError> = invoke_with_retry(
        || async { Ok(()) },
        |()| {
            let attempts = attempts_probe.clone();
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                Ok("allowed")
            }
        },
    )
    .await;

    assert_eq!(result.unwrap(), "allowed");
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}
