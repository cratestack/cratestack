use std::sync::Arc;
use std::time::Duration;

use axum::extract::Request;
use cratestack_core::CratestackError;
use cratestack_exec::{OpAdmission, OpExecutor};
use tower::Layer;

use super::budget::RateLimitBucketBudget;
use super::budget::warn::BudgetWarnings;
use super::config::RateLimitConfig;
use super::key_fn::default_key_fn;
use super::policy::{DEFAULT_STORE_TIMEOUT, StoreErrorPolicy, StoreErrorWarnings};
use super::scope::{KeyDerivation, UnverifiedAuthPolicy};
use super::service::RateLimitService;
use super::store::RateLimitStore;

pub(super) type KeyFn =
    Arc<dyn Fn(&Request) -> Result<KeyDerivation, CratestackError> + Send + Sync>;
pub(super) type OpResolver = Arc<dyn Fn(&Request) -> OpAdmission + Send + Sync>;

#[derive(Clone)]
pub struct RateLimitLayer {
    store: Arc<dyn RateLimitStore>,
    config: RateLimitConfig,
    key_fn: Option<KeyFn>,
    op_resolver: OpResolver,
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
            // Every request unidentified — and so charged — until a
            // resolver says otherwise: rate limiting's fail-closed default.
            op_resolver: Arc::new(|_| OpAdmission::unresolved()),
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

    /// Exempt requests by predicate: `false` skips the limiter — key
    /// derivation included. Kept as a thin adapter over
    /// [`with_op_resolver`](Self::with_op_resolver) since ADR 0015 slice 2
    /// (cratestack#877) moved the decision to [`OpExecutor`]; the
    /// `idempotent_by_default` it fills in is never read by this layer.
    pub fn with_should_rate_limit_fn(
        self,
        f: impl Fn(&Request) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.with_op_resolver(move |req| OpAdmission::new("", false, f(req)))
    }

    /// Tell the layer which op a request is, so `@no_rate_limit` is
    /// honoured. Takes the same resolvers as
    /// [`crate::idempotency::IdempotencyLayer::with_op_resolver`] — pass
    /// [`crate::idempotency::build_rpc_op_resolver_with_prefix`] (or its
    /// REST twin) to cover a router mounted with `Router::nest`, which the
    /// `build_*_ops_filter` predicates cannot. A resolver miss answers
    /// [`OpAdmission::unresolved`], which is rate limited.
    pub fn with_op_resolver(
        mut self,
        f: impl Fn(&Request) -> OpAdmission + Send + Sync + 'static,
    ) -> Self {
        self.op_resolver = Arc::new(f);
        self
    }

    /// Test seam: the warning counters this layer shares with every
    /// service it builds. Lets a test assert that the per-request path
    /// actually *called* `consume::report`, which deleting outright used
    /// to leave every test green (cratestack#871 review, should-fix 3).
    pub(super) fn _budget_warnings(&self) -> &BudgetWarnings {
        &self.budget_warnings
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
            // No idempotency store: this executor only ever answers the
            // rate-limit question, so the reservation TTL is never read.
            executor: OpExecutor::new(None, Duration::ZERO)
                .with_rate_limit(self.store.clone(), self.config),
            config: self.config,
            key_fn: self.resolved_key_fn(),
            op_resolver: self.op_resolver.clone(),
            store_error_policy: self.store_error_policy,
            store_timeout: self.store_timeout,
            warnings: self.warnings.clone(),
            budget_warnings: self.budget_warnings.clone(),
        }
    }
}
