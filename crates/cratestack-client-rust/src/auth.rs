use crate::error::ClientError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    pub method: String,
    pub path: String,
    pub canonical_query: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub canonical_request: String,
}

/// Async by design (issue #453): real credential providers — an OAuth2
/// client-credentials token with a refresh-on-expiry cache, for instance —
/// need to make an HTTP call on a cache miss, which a sync `authorize`
/// can't do without `block_on` (panics/deadlocks depending on the runtime)
/// or a pre-fetch-and-stash workaround that reintroduces the expiry race
/// the cache existed to avoid. `#[async_trait]` (rather than a bare
/// `async fn` in the trait) keeps `Arc<dyn RequestAuthorizer>` —
/// `CratestackClient::with_request_authorizer` stores the authorizer as
/// a trait object — dyn-compatible; native AFIT would drop that. Same
/// convention other dyn-dispatched async hook traits already use elsewhere
/// in the workspace (e.g. `cratestack_core::audit::AuditSink`).
///
/// Target-split (cratestack#1104 follow-up): natively the trait is
/// `Send + Sync` with a `Send` future, exactly as before. On
/// `wasm32-unknown-unknown` it is neither, because a browser `fetch` future
/// is never `Send`, so an authorizer that makes a request of its own (the
/// token refresh above) could not satisfy a `Send` bound there. An
/// implementation that builds for both targets uses the same split:
/// `#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]` and
/// `#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]`.
#[cfg(not(target_arch = "wasm32"))]
#[async_trait::async_trait]
pub trait RequestAuthorizer: Send + Sync {
    async fn authorize(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError>;
}

/// The wasm32 half of [`RequestAuthorizer`]: no `Send`/`Sync`, and a
/// future that need not be `Send`, so `authorize` can await a `fetch`.
#[cfg(target_arch = "wasm32")]
#[async_trait::async_trait(?Send)]
pub trait RequestAuthorizer {
    async fn authorize(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError>;
}
