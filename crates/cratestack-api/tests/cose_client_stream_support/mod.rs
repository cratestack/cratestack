//! Shared by `cose_client_stream_{rpc,rest}.rs` (cratestack#1378): the
//! generated Rust client opening a `@frames(unsigned)` subscription over the
//! signed transport, against the generated server behind its envelope layer,
//! with the tampering proxy of `cose_client_support` in between. The cases
//! are one macro both transports instantiate, so they cannot drift.

// Each test binary uses a different subset of these helpers.
#![allow(dead_code, unused_imports)]

mod cases;
mod refusal_cases;

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use cratestack::{CratestackContext, CratestackError, RequestContext};
use cratestack_client_rust::{ClientError, EnvelopeError, RpcClientError};

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

/// Why an open, or a stream, failed: the same on both transports, whose
/// generated clients return different error types.
#[derive(Debug)]
pub enum Fail {
    Envelope(EnvelopeError),
    Remote { status: u16, message: String },
    Other(String),
}

impl From<ClientError> for Fail {
    fn from(error: ClientError) -> Self {
        match error {
            ClientError::Envelope(error) => Fail::Envelope(error),
            ClientError::Remote {
                status, message, ..
            } => Fail::Remote {
                status: status.as_u16(),
                message,
            },
            other => Fail::Other(other.to_string()),
        }
    }
}

impl From<RpcClientError> for Fail {
    fn from(error: RpcClientError) -> Self {
        match error {
            RpcClientError::Envelope(error) => Fail::Envelope(error),
            RpcClientError::Remote(remote) => Fail::Remote {
                status: remote.status.as_u16(),
                message: remote.body.message,
            },
            other => Fail::Other(other.to_string()),
        }
    }
}

/// A subscription read to its end: the ticks it carried and how it ended
/// (`None`: cleanly, with an `end` frame).
#[derive(Debug)]
pub struct Drained {
    pub ticks: Vec<i64>,
    pub ended: Option<Fail>,
}

/// Reads `subscription` until it closes.
pub async fn drain<T>(
    mut subscription: cratestack_client_rust::ClientSubscription<T>,
    tick: fn(T) -> i64,
) -> Drained {
    let (mut ticks, mut ended) = (Vec::new(), None);
    while let Some(item) = subscription.recv().await {
        match item {
            Ok(item) => ticks.push(tick(item)),
            Err(error) => ended = Some(Fail::from(error)),
        }
    }
    Drained { ticks, ended }
}
