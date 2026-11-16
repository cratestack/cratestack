//! What the signed subscription tests share besides the sealed open: the
//! handler counter, the authenticating provider and frame readers.

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use cratestack::{CratestackContext, CratestackError, RequestContext};

use crate::cose_support::SIGN1;

use super::STREAM;

/// A source that yields `tick(n)` every 100 ms, for ever: the fixture
/// registry's answer to `to < 0`.
pub fn endless_ticks<T: Send + 'static>(tick: fn(i64) -> T) -> cratestack::SubscriptionStream<T> {
    Box::pin(cratestack::futures::stream::unfold(
        0,
        move |n| async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            Some((Ok(tick(n)), n + 1))
        },
    ))
}

/// What the registry counts: how often a handler ran.
pub type Hits = Arc<AtomicUsize>;

/// Authenticates everyone, or no one (the `@allow` refusal).
#[derive(Clone)]
pub struct Caller(pub bool);

impl cratestack::AuthProvider for Caller {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(match self.0 {
            true => {
                CratestackContext::authenticated([("id".to_owned(), cratestack::Value::Int(1))])
            }
            false => CratestackContext::anonymous(),
        }))
    }
}

pub fn both_accepts() -> String {
    format!("{STREAM}, {SIGN1}")
}

/// The ticks a decoded cbor-seq body carried, and whether it ended cleanly.
pub fn ticks(body: &[u8]) -> (usize, bool) {
    let frames = crate::cbor_seq_support::read(body).frames;
    let nexts = frames.iter().filter(|frame| frame.next.is_some()).count();
    (
        nexts,
        frames.last().is_some_and(|frame| frame.end.is_some()),
    )
}
