//! The retry itself: exactly once, only for transport-class failures,
//! and never for a deterministic refusal (cratestack#846).
//!
//! A real broken pipe cannot be induced deterministically against a
//! testcontainer — `CLIENT KILL` races the manager's background
//! reconnect, so the test would be flaky in exactly the way that teaches
//! people to ignore it. These drive the *shipped* retry function
//! (`super::retry::invoke_with_retry`, the same one `consume` calls) with
//! an injected connection that fails once, which is what the production
//! incident actually looked like from this code's side.

#![cfg(test)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cratestack_core::CratestackError;

use super::retry::invoke_with_retry;
use super::tests_error_class::{always_warn, broken_pipe, oom_error};

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
        &always_warn(),
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
        "the retry must re-enter connection(), so that a store whose FIRST connect failed (the \
         OnceCell caches no Err) gets a second chance to establish one at all"
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
        &always_warn(),
    )
    .await;

    let error = result.expect_err("a store that is genuinely down must still fail");
    assert!(
        matches!(error, CratestackError::Unavailable(_)),
        "a store that never answered is transport-class: {error}"
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
                Err(oom_error())
            }
        },
        &always_warn(),
    )
    .await;

    let error = result.expect_err("an OOM must fail the call");
    assert!(
        matches!(error, CratestackError::Internal(_)),
        "an OOM must stay Internal so the layer refuses it even under Allow: {error}"
    );
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "an OOM/NOSCRIPT reply is deterministic; retrying it only doubles the latency"
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
        &always_warn(),
    )
    .await;

    assert_eq!(result.unwrap(), "allowed");
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}
