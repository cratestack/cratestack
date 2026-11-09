//! Rate-limiting subscription opens on the session bindings
//! (`docs/design/subscriptions-websocket.md` §6.7): the seam WebSocket and
//! WebTransport call, implemented by `cratestack-axum`'s
//! `SubscriptionRateLimiter` over `OpExecutor::admit_rate_limit`. One token
//! per subscription `Request`; the session itself is not charged.

use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;

use super::frame::ServerFrame;
use crate::CratestackError;

/// What a binding knows about one subscription open.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct SubscriptionCharge<'a> {
    pub op_id: &'a str,
    /// The session's principal key (`PrincipalKeyFn`); `None` = anonymous.
    pub principal: Option<&'a str>,
    /// The verified peer; `None` when the binding has none (WebSocket served
    /// without `ConnectInfo`).
    pub remote: Option<IpAddr>,
}

impl<'a> SubscriptionCharge<'a> {
    pub fn new(op_id: &'a str, principal: Option<&'a str>, remote: Option<IpAddr>) -> Self {
        Self {
            op_id,
            principal,
            remote,
        }
    }
}

/// Why an open was refused before `Ready`.
#[derive(Debug)]
#[non_exhaustive]
pub enum SubscriptionRefusal {
    /// The caller's bucket is empty.
    Throttled { retry_after_secs: u32 },
    /// Anything else: no identity to key on, or a store failure the
    /// configured policy refuses.
    Error(CratestackError),
}

impl SubscriptionRefusal {
    /// The `{id, err}` frame. A throttle is `resource_exhausted` with
    /// `details: {"retry_after_secs": N}`: frames have no headers, so this is
    /// how `Retry-After` reaches the client.
    pub fn frame(&self, id: u64) -> ServerFrame {
        match self {
            Self::Throttled { retry_after_secs } => {
                let error = CratestackError::TooManyRequests("rate limit exceeded".to_owned());
                let mut frame = ServerFrame::err(id, &error);
                if let Some(body) = frame.err.as_mut() {
                    body.details =
                        Some(serde_json::json!({ "retry_after_secs": retry_after_secs }));
                }
                frame
            }
            Self::Error(error) => ServerFrame::err(id, error),
        }
    }
}

/// Charges one subscription open. `Debug` so binding configs keep deriving it.
#[async_trait]
pub trait SubscriptionRateLimit: fmt::Debug + Send + Sync + 'static {
    async fn admit(&self, charge: SubscriptionCharge<'_>) -> Result<(), SubscriptionRefusal>;
}

/// One session's standing answer to "who pays": built once per session and
/// cloned into every subscription task. With no limiter it admits everything.
#[derive(Debug, Clone, Default)]
pub struct SessionCharger {
    limiter: Option<Arc<dyn SubscriptionRateLimit>>,
    principal: Option<String>,
    remote: Option<IpAddr>,
}

impl SessionCharger {
    pub fn new(
        limiter: Option<Arc<dyn SubscriptionRateLimit>>,
        principal: Option<String>,
        remote: Option<IpAddr>,
    ) -> Self {
        Self {
            limiter,
            principal,
            remote,
        }
    }

    pub fn principal(&self) -> Option<&str> {
        self.principal.as_deref()
    }

    pub async fn admit(&self, op_id: &str) -> Result<(), SubscriptionRefusal> {
        match &self.limiter {
            None => Ok(()),
            Some(limiter) => {
                let charge = SubscriptionCharge::new(op_id, self.principal.as_deref(), self.remote);
                limiter.admit(charge).await
            }
        }
    }
}

#[cfg(test)]
#[path = "tests_rate_limit.rs"]
mod tests;
