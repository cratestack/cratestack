//! [`crate::StoreErrorPolicy::permits`] is the rule HTTP and MCP now share
//! (cratestack#1038), so it is pinned here, where both read it, and not
//! only through one transport's tests.

use cratestack_core::CratestackError;

use crate::StoreErrorPolicy;

fn failures() -> [CratestackError; 4] {
    [
        CratestackError::Unavailable("connection refused".to_owned()),
        CratestackError::Internal("OOM command not allowed".to_owned()),
        CratestackError::Forbidden("NOPERM".to_owned()),
        CratestackError::TooManyRequests("a 429 is not a transport failure".to_owned()),
    ]
}

#[test]
fn the_default_is_allow() {
    assert_eq!(StoreErrorPolicy::default(), StoreErrorPolicy::Allow);
}

#[test]
fn allow_serves_through_unavailable_and_nothing_else() {
    let served: Vec<bool> = failures()
        .iter()
        .map(|error| StoreErrorPolicy::Allow.permits(error))
        .collect();
    assert_eq!(served, [true, false, false, false]);
}

#[test]
fn deny_serves_through_nothing_not_even_unavailable() {
    for error in failures() {
        assert!(!StoreErrorPolicy::Deny.permits(&error), "{error:?}");
    }
}
