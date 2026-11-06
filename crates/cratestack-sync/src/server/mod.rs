//! Server half: change log, push replay, pull, TTL GC and the poke stream.
//!
//! ```text
//! let server = SyncServer::builder(pool, registry, resolver)
//!     .policy(SyncPolicy::default())
//!     .rate_limit(store, RateLimitConfig::new(30, 1.0))
//!     .build()?;
//! server.install().await?;                    // tables + record()
//! server.capture(CaptureSpec { .. }).await?;  // one trigger per synced table
//! server.spawn_background();                  // LISTEN/NOTIFY + GC loop
//! let app = Router::new().merge(sync_router(server, auth_provider));
//! ```

mod clients;
mod ddl;
mod gc;
mod http;
mod poke;
mod principal;
mod pull;
mod push;
mod registry;

use std::sync::Arc;

use cratestack_core::{CratestackContext, RateLimitConfig, RateLimitDecision, RateLimitStore};
use cratestack_sqlx::sqlx::{self, PgPool};

pub use ddl::{CaptureSpec, IdentifierError, SERVER_DDL, capture_trigger_sql};
pub use gc::GcReport;
pub use http::sync_router;
pub use poke::PokeHub;
pub use principal::subject_resolver;
pub use registry::{MutationContext, MutationError, MutationHandler, RowFetcher, SyncRegistry};

use crate::policy::{PolicyError, SyncPolicy};
use crate::protocol::{SyncErrorBody, SyncErrorCode};

/// Request-level failure, serialized to the client as-is.
pub type SyncError = SyncErrorBody;

/// The authenticated caller as the sync engine sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPrincipal {
    /// Stable identity a client id is bound to.
    pub id: String,
    /// Every scope this principal may pull and be poked for. Server-derived
    /// from the auth context — a client never names a scope.
    pub scopes: Vec<String>,
}

/// Maps an authenticated context to a principal; `None` refuses sync.
/// [`subject_resolver`] is the default shape: the id is `ctx.subject()`.
pub type ScopeResolver = Arc<dyn Fn(&CratestackContext) -> Option<SyncPrincipal> + Send + Sync>;

#[derive(Clone)]
pub struct SyncServer {
    pub(crate) pool: PgPool,
    pub(crate) policy: SyncPolicy,
    pub(crate) registry: Arc<SyncRegistry>,
    pub(crate) resolver: ScopeResolver,
    pub(crate) rate_limit: Option<(Arc<dyn RateLimitStore>, RateLimitConfig)>,
    pub(crate) pokes: PokeHub,
}

pub struct SyncServerBuilder {
    server: SyncServer,
}

impl SyncServer {
    pub fn builder(
        pool: PgPool,
        registry: SyncRegistry,
        resolver: impl Fn(&CratestackContext) -> Option<SyncPrincipal> + Send + Sync + 'static,
    ) -> SyncServerBuilder {
        SyncServerBuilder {
            server: SyncServer {
                pool,
                policy: SyncPolicy::default(),
                registry: Arc::new(registry),
                resolver: Arc::new(resolver),
                rate_limit: None,
                pokes: PokeHub::new(),
            },
        }
    }

    pub fn policy(&self) -> &SyncPolicy {
        &self.policy
    }

    pub fn pokes(&self) -> &PokeHub {
        &self.pokes
    }

    /// Create the sync tables and functions. Idempotent.
    pub async fn install(&self) -> Result<(), sqlx::Error> {
        sqlx::raw_sql(SERVER_DDL).execute(&self.pool).await?;
        Ok(())
    }

    /// Install the capture trigger on one table. Idempotent.
    pub async fn capture(&self, spec: CaptureSpec<'_>) -> Result<(), CaptureError> {
        let sql = capture_trigger_sql(spec)?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) fn principal(&self, ctx: &CratestackContext) -> Result<SyncPrincipal, SyncError> {
        (self.resolver)(ctx).ok_or_else(|| {
            SyncError::new(SyncErrorCode::Unauthenticated, "sync requires a principal")
        })
    }

    /// Per-principal token bucket, keyed by operation.
    pub(crate) async fn admit(&self, op: &str, principal: &str) -> Result<(), SyncError> {
        let Some((store, config)) = &self.rate_limit else {
            return Ok(());
        };
        let key = format!("sync:{op}:{principal}");
        match store.consume(&key, *config).await {
            Ok(RateLimitDecision::Allowed { .. }) => Ok(()),
            Ok(RateLimitDecision::Throttled { retry_after_secs }) => {
                let mut error = SyncError::new(SyncErrorCode::RateLimited, "slow down");
                error.retry_after_secs = Some(retry_after_secs);
                Err(error)
            }
            // Fail closed: an unreachable limiter must not become "unlimited".
            Err(error) => Err(internal(error)),
        }
    }
}

impl SyncServerBuilder {
    pub fn policy(mut self, policy: SyncPolicy) -> Self {
        self.server.policy = policy;
        self
    }

    /// Rate-limit every sync operation per principal. Any
    /// [`RateLimitStore`] works — the in-memory one from `cratestack-axum`
    /// or the Redis one from `cratestack-redis`.
    pub fn rate_limit(mut self, store: Arc<dyn RateLimitStore>, config: RateLimitConfig) -> Self {
        self.server.rate_limit = Some((store, config));
        self
    }

    pub fn build(self) -> Result<SyncServer, PolicyError> {
        self.server.policy.validate()?;
        Ok(self.server)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

pub(crate) fn internal(error: impl std::fmt::Display) -> SyncError {
    // Detail goes to the log, not the wire.
    tracing::error!(%error, "sync request failed");
    SyncError::new(SyncErrorCode::Internal, "internal error")
}
