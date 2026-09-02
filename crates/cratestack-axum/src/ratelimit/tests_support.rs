//! Shared fixtures for the store-error/typed-body test modules: a store
//! that always fails, and a `tracing` layer that captures events so a
//! test can assert on log level and content without a full fmt layer.
//!
//! Moved verbatim out of `tests.rs` when cratestack#846 split the
//! store-error tests into their own module.

#![cfg(test)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::to_bytes;
use axum::response::Response;
use cratestack_core::{CratestackError, RateLimitConfig, RateLimitDecision};
use http::header;

use super::store::RateLimitStore;

/// A store that always fails, standing in for e.g. an unreachable Redis
/// backend behind `RedisRateLimitStore`. The message is the one from the
/// production incident in cratestack#846.
pub(super) struct FailingStore;

#[async_trait]
impl RateLimitStore for FailingStore {
    async fn consume(
        &self,
        _key: &str,
        _config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        Err(CratestackError::Internal(
            "redis rate limit: connection refused".to_owned(),
        ))
    }
}

/// Records every event's level + fields as a formatted string, so a test can
/// assert on log level and content without a full `tracing-subscriber` fmt layer.
#[derive(Default, Clone)]
pub(super) struct CapturingLayer {
    pub(super) events: Arc<Mutex<Vec<(tracing::Level, String)>>>,
}

pub(super) struct FieldsToString(pub(super) String);

impl tracing::field::Visit for FieldsToString {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        self.0.push_str(&format!("{}={value:?}", field.name()));
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CapturingLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = FieldsToString(String::new());
        event.record(&mut visitor);
        self.events
            .lock()
            .unwrap()
            .push((*event.metadata().level(), visitor.0));
    }
}

/// Buffer a response into `(content_type, body_bytes)`.
pub(super) async fn content_type_and_body(response: Response) -> (String, Vec<u8>) {
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body should buffer");
    (content_type, bytes.to_vec())
}
