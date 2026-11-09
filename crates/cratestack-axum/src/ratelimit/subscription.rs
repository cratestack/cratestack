//! Rate-limiting WebSocket / WebTransport subscription opens
//! (`docs/design/subscriptions-websocket.md` §6.7). The bindings call the
//! core trait [`SubscriptionRateLimit`]; this implements it over
//! [`OpExecutor::admit_rate_limit`] with the HTTP layer's own pieces, so the
//! store, token bucket, [`StoreErrorPolicy`] and lookup timeout are shared.

use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cratestack_core::subscription_wire::{
    SubscriptionCharge, SubscriptionRateLimit, SubscriptionRefusal,
};
use cratestack_core::{BoundedOutcome, CratestackError, RateLimitDecision};
use cratestack_exec::{OpAdmission, OpExecutor, OpInput, RateLimitAdmission, RateLimitBucket};

use super::config::RateLimitConfig;
use super::key_fn::sha256_hex;
use super::layer::RateLimitLayer;
use super::policy::{DEFAULT_STORE_TIMEOUT, StoreErrorPolicy, StoreErrorWarnings};
use super::scope::bucket_address;
use super::store::RateLimitStore;
use super::store_error::{StoreFailure, classify_store_failure};

type OpResolver = Arc<dyn Fn(&str) -> OpAdmission + Send + Sync>;

/// Charges one token per subscription open. Build it with
/// [`RateLimitLayer::for_subscriptions`] (sharing that layer's store and
/// settings) or [`SubscriptionRateLimiter::new`], and hand it to
/// `WebSocketConfig::rate_limit` / `WebTransportConfig::rate_limit`.
///
/// The bucket key follows the HTTP layer's precedence on an already
/// verified session: `princ:<sha256 of the principal key>`, else
/// `ip:<peer>` (IPv6 aggregated to its /64), else the open is refused
/// `failed_precondition` (HTTP's 412 rule, cratestack#416). No `auth:` key
/// is ever minted: identity is verified before admission.
#[derive(Clone)]
pub struct SubscriptionRateLimiter {
    executor: OpExecutor,
    op_resolver: OpResolver,
    store_error_policy: StoreErrorPolicy,
    store_timeout: Duration,
    warnings: Arc<StoreErrorWarnings>,
}

impl SubscriptionRateLimiter {
    pub fn new(store: Arc<dyn RateLimitStore>, config: RateLimitConfig) -> Self {
        Self {
            executor: OpExecutor::new(None, Duration::ZERO).with_rate_limit(store, config),
            // Every open charged until a resolver says otherwise: fail closed.
            op_resolver: Arc::new(|_| OpAdmission::unresolved()),
            store_error_policy: StoreErrorPolicy::default(),
            store_timeout: DEFAULT_STORE_TIMEOUT,
            warnings: Arc::new(StoreErrorWarnings::default()),
        }
    }

    pub fn with_store_error_policy(mut self, policy: StoreErrorPolicy) -> Self {
        self.store_error_policy = policy;
        self
    }

    pub fn with_store_timeout(mut self, timeout: Duration) -> Self {
        self.store_timeout = timeout;
        self
    }

    /// Which ops are charged, by op id. The default charges all of them,
    /// because subscription ops cannot be `@no_rate_limit`; this hook exists
    /// for parity with [`RateLimitLayer::with_op_resolver`].
    pub fn with_op_resolver(
        mut self,
        resolver: impl Fn(&str) -> OpAdmission + Send + Sync + 'static,
    ) -> Self {
        self.op_resolver = Arc::new(resolver);
        self
    }
}

impl RateLimitLayer {
    /// A limiter for WebSocket / WebTransport subscription opens with this
    /// layer's store, token bucket, [`StoreErrorPolicy`], lookup timeout and
    /// warning throttle. A principal's bucket is the one its HTTP requests
    /// use when the application's `VerifiedPrincipal` string equals the
    /// binding's principal key (not so for COSE's `cose:<thumbprint>`).
    pub fn for_subscriptions(&self) -> SubscriptionRateLimiter {
        SubscriptionRateLimiter {
            executor: OpExecutor::new(None, Duration::ZERO)
                .with_rate_limit(self.store.clone(), self.config),
            op_resolver: Arc::new(|_| OpAdmission::unresolved()),
            store_error_policy: self.store_error_policy,
            store_timeout: self.store_timeout,
            warnings: self.warnings.clone(),
        }
    }
}

impl fmt::Debug for SubscriptionRateLimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubscriptionRateLimiter")
            .field("store_timeout", &self.store_timeout)
            .field("store_error_policy", &self.store_error_policy)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl SubscriptionRateLimit for SubscriptionRateLimiter {
    async fn admit(&self, charge: SubscriptionCharge<'_>) -> Result<(), SubscriptionRefusal> {
        let op = (self.op_resolver)(charge.op_id);
        if !self.executor.rate_limit_applies(&op) {
            return Ok(());
        }
        let key =
            bucket_key(charge.principal, charge.remote).map_err(SubscriptionRefusal::Error)?;
        let input = OpInput::for_rate_limit(op, RateLimitBucket::new(&key, None));
        let admitted =
            tokio::time::timeout(self.store_timeout, self.executor.admit_rate_limit(&input))
                .await
                .unwrap_or_else(|_| Err(super::policy::store_timeout_error()));
        match admitted {
            Ok(RateLimitAdmission::Bypass) => Ok(()),
            Ok(RateLimitAdmission::Consumed(BoundedOutcome { decision, .. })) => match decision {
                RateLimitDecision::Allowed { .. } => Ok(()),
                RateLimitDecision::Throttled { retry_after_secs } => {
                    Err(SubscriptionRefusal::Throttled { retry_after_secs })
                }
            },
            // `#[non_exhaustive]`: an outcome this build does not know must
            // refuse, never open the subscription.
            Ok(_) => Err(SubscriptionRefusal::Error(CratestackError::Internal(
                "rate limit: unhandled admission outcome; refusing rather than opening the \
                 subscription"
                    .to_owned(),
            ))),
            Err(error) => {
                match classify_store_failure(error, self.store_error_policy, &self.warnings) {
                    StoreFailure::Serve => Ok(()),
                    StoreFailure::Refuse(error) => Err(SubscriptionRefusal::Error(error)),
                }
            }
        }
    }
}

/// HTTP's precedence (`key_fn.rs`) on a verified session: principal, then
/// peer, else 412.
fn bucket_key(principal: Option<&str>, remote: Option<IpAddr>) -> Result<String, CratestackError> {
    if let Some(principal) = principal.filter(|p| !p.is_empty()) {
        return Ok(format!("princ:{}", sha256_hex(principal.as_bytes())));
    }
    if let Some(ip) = remote {
        return Ok(format!("ip:{}", bucket_address(ip)));
    }
    Err(CratestackError::PreconditionFailed(
        "rate limit: an anonymous subscription session has no peer address to key its bucket \
         on; serve the router through into_make_service_with_connect_info::<SocketAddr>()"
            .to_owned(),
    ))
}
