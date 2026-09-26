//! How the application tunes L3 admission on a server. Split from
//! `server.rs` for the file-length ceiling.

use cratestack_exec::{OpExecutor, StoreErrorPolicy};

use super::McpServer;
use crate::table::McpTools;

impl<T: McpTools> McpServer<T> {
    /// Opt in to L3 admission: an executor built with an idempotency store
    /// and/or `with_rate_limit`, as the application would build one for
    /// `cratestack-axum`'s layers.
    pub fn with_executor(mut self, executor: OpExecutor) -> Self {
        self.executor = executor;
        self
    }

    /// Choose what a failing rate-limit store does to a call, as
    /// `RateLimitLayer::with_store_error_policy` does on HTTP; pass the
    /// same value to both. Defaults to [`StoreErrorPolicy::Allow`], HTTP's
    /// default: serve through a transport-class failure (`Unavailable`,
    /// including a lookup that outlives `DEFAULT_STORE_TIMEOUT`), refuse
    /// every other. [`StoreErrorPolicy::Deny`] refuses them all, for a
    /// limiter that is a security control rather than a capacity one.
    /// Unread without a rate limiter on the executor.
    pub fn with_store_error_policy(mut self, policy: StoreErrorPolicy) -> Self {
        self.store_error_policy = policy;
        self
    }
}
