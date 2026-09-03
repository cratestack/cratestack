use std::sync::Arc;
use std::time::Duration;

use axum::extract::Request;
use cratestack_core::CratestackError;
use tower::Layer;

use super::budget::RateLimitBucketBudget;
use super::budget::warn::BudgetWarnings;
use super::config::RateLimitConfig;
use super::key_fn::{default_key_fn, default_should_rate_limit_fn};
use super::policy::{DEFAULT_STORE_TIMEOUT, StoreErrorPolicy, StoreErrorWarnings};
use super::scope::{KeyDerivation, UnverifiedAuthPolicy};
use super::service::RateLimitService;
use super::store::RateLimitStore;

pub(super) type KeyFn =
    Arc<dyn Fn(&Request) -> Result<KeyDerivation, CratestackError> + Send + Sync>;

#[derive(Clone)]
pub struct RateLimitLayer {
    store: Arc<dyn RateLimitStore>,
    config: RateLimitConfig,
    key_fn: Option<KeyFn>,
    should_rate_limit_fn: Arc<dyn Fn(&Request) -> bool + Send + Sync>,
    store_error_policy: StoreErrorPolicy,
    store_timeout: Duration,
    bucket_budget: Option<RateLimitBucketBudget>,
    unverified_auth_policy: UnverifiedAuthPolicy,
    warnings: Arc<StoreErrorWarnings>,
    budget_warnings: Arc<BudgetWarnings>,
}

impl RateLimitLayer {
    pub fn new(store: Arc<dyn RateLimitStore>, config: RateLimitConfig) -> Self {
        Self {
            store,
            config,
            key_fn: None,
            should_rate_limit_fn: Arc::new(default_should_rate_limit_fn),
            store_error_policy: StoreErrorPolicy::default(),
            store_timeout: DEFAULT_STORE_TIMEOUT,
            bucket_budget: Some(RateLimitBucketBudget::default()),
            unverified_auth_policy: UnverifiedAuthPolicy::default(),
            warnings: Arc::new(StoreErrorWarnings::default()),
            budget_warnings: Arc::new(BudgetWarnings::default()),
        }
    }

    /// Choose what happens when the backing store itself fails, as
    /// opposed to when a caller is genuinely over budget. Defaults to
    /// [`StoreErrorPolicy::Allow`], which serves through **transport-class
    /// failures only** — see that type's docs for the distinction, why a
    /// reachable-but-refusing store stays closed regardless, and why key
    /// derivation deliberately does not follow suit.
    pub fn with_store_error_policy(mut self, policy: StoreErrorPolicy) -> Self {
        self.store_error_policy = policy;
        self
    }

    /// Ceiling on how long one store lookup may take before the layer
    /// gives up and applies [`StoreErrorPolicy`] to a synthetic
    /// transport-class error. Defaults to [`DEFAULT_STORE_TIMEOUT`].
    ///
    /// This is ONE budget for the whole lookup, including any retry the
    /// backend performs internally — the point is to bound what the
    /// caller waits, and a per-attempt budget silently doubles when a
    /// store retries. Without it, "degrade to unlimited" degrades only
    /// after the driver's own reconnect cycle finishes, which was
    /// measured at nineteen seconds per request against a real outage.
    pub fn with_store_timeout(mut self, timeout: Duration) -> Self {
        self.store_timeout = timeout;
        self
    }

    /// Tune how many distinct buckets one scope may create
    /// (cratestack#871). Defaults to [`RateLimitBucketBudget::default`].
    pub fn with_bucket_budget(mut self, budget: RateLimitBucketBudget) -> Self {
        self.bucket_budget = Some(budget);
        self
    }

    /// Let an unverified `Authorization` header mint buckets without any
    /// cardinality bound — the pre-cratestack#871 behaviour.
    ///
    /// Only correct when something else already bounds the keyspace (an
    /// authenticating proxy in front, a `with_key_fn` that keys on
    /// verified material, mTLS). Otherwise this restores the measured
    /// amplification primitive: one store key per request, attacker-chosen.
    pub fn without_bucket_budget(mut self) -> Self {
        self.bucket_budget = None;
        self
    }

    /// What the default key function does with an `Authorization` header
    /// nothing has verified. See [`UnverifiedAuthPolicy`].
    pub fn with_unverified_auth_policy(mut self, policy: UnverifiedAuthPolicy) -> Self {
        self.unverified_auth_policy = policy;
        self
    }

    /// Override how the layer derives the bucket key. The supplied closure
    /// is infallible by design — opting out of the default's fail-closed
    /// behavior is the caller's explicit choice, including any deliberate
    /// shared bucket.
    ///
    /// An override carries **no** bucket budget: the layer has no basis to
    /// invent a scope or a fallback for a key whose derivation it cannot
    /// see. A consumer whose key function reads caller-supplied material
    /// owns bounding it, exactly as it owns the fail-closed decision.
    pub fn with_key_fn(mut self, f: impl Fn(&Request) -> String + Send + Sync + 'static) -> Self {
        self.key_fn = Some(Arc::new(move |req| Ok(KeyDerivation::unbudgeted(f(req)))));
        self
    }

    pub fn with_should_rate_limit_fn(
        mut self,
        f: impl Fn(&Request) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.should_rate_limit_fn = Arc::new(f);
        self
    }

    /// Bind the layer's configuration into the closure `RateLimitService`
    /// calls, so the per-request path never has to branch on "default or
    /// override" again.
    fn resolved_key_fn(&self) -> KeyFn {
        if let Some(key_fn) = &self.key_fn {
            return key_fn.clone();
        }
        let budget = self.bucket_budget;
        let policy = self.unverified_auth_policy;
        let warnings = self.budget_warnings.clone();
        Arc::new(move |req| match budget {
            Some(budget) => default_key_fn(req, budget, policy, &warnings),
            // `without_bucket_budget()`: derive exactly as before, then
            // drop the budget rather than skipping derivation, so the
            // key SHAPE (and therefore every existing bucket) is
            // untouched by the opt-out.
            None => default_key_fn(req, RateLimitBucketBudget::default(), policy, &warnings)
                .map(|derivation| KeyDerivation::unbudgeted(derivation.key)),
        })
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            store: self.store.clone(),
            config: self.config,
            key_fn: self.resolved_key_fn(),
            should_rate_limit_fn: self.should_rate_limit_fn.clone(),
            store_error_policy: self.store_error_policy,
            store_timeout: self.store_timeout,
            warnings: self.warnings.clone(),
            budget_warnings: self.budget_warnings.clone(),
        }
    }
}
