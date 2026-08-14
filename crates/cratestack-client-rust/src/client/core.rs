use std::sync::Arc;

use cratestack_codec_cbor::CborCodec;

use crate::auth::RequestAuthorizer;
use crate::codec::HttpClientCodec;
use crate::config::ClientConfig;
use crate::error::ClientError;
use crate::state::{ClientStateStore, InMemoryStateStore, PersistedClientState};

/// Installs a `ring`-backed `rustls::crypto::CryptoProvider` if the process
/// doesn't already have one (#440).
///
/// `reqwest`'s `rustls-no-provider` feature — deliberately chosen over
/// `rustls` so this crate stops forcing `aws-lc-rs` on every consumer of
/// `cratestack-pg` (see the workspace `Cargo.toml`'s `reqwest` entry) — ships
/// no crypto provider at all: `reqwest::Client::new()`/`ClientBuilder::build()`
/// PANIC at construction time if `rustls::crypto::CryptoProvider::get_default()`
/// finds nothing installed. Unlike the old `rustls` feature's silent
/// `aws-lc-rs` install, that's a worse zero-config default than what this
/// crate had before, not merely a neutral one.
///
/// `install_default()` only ever takes effect the FIRST time it succeeds
/// process-wide — it's a courtesy fallback, not an override. A consumer that
/// installs its own provider (any backend, including `aws-lc-rs`) before
/// constructing its first `CratestackClient` keeps that choice; this only
/// fires when nobody has chosen anything yet, which is exactly the gap
/// `rustls-no-provider` otherwise turns into a panic. The `Err` it returns
/// on a race with another caller installing first (or a no-op call to this
/// same function from a second `CratestackClient::new`) is expected and
/// intentionally ignored.
fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[derive(Clone)]
pub struct CratestackClient<C = CborCodec> {
    pub(crate) http: reqwest::Client,
    pub(crate) config: ClientConfig,
    pub(crate) codec: C,
    pub(crate) state_store: Arc<dyn ClientStateStore>,
    pub(crate) request_authorizer: Option<Arc<dyn RequestAuthorizer>>,
    /// The generating schema's `SCHEMA_SHA256` (issue #178) — `None` for a
    /// hand-constructed client that isn't wrapped by generated `Client`
    /// code (e.g. a bare `CratestackClient` in a test). Set automatically
    /// by the schema-generated `Client::new` wrapper, never by the schema
    /// author directly. Sent as `x-cratestack-schema-sha` on every request
    /// when present; the server-side counterpart only ever warns on
    /// mismatch, never rejects.
    pub(crate) schema_sha: Option<&'static str>,
}

impl CratestackClient<CborCodec> {
    pub fn cbor(config: ClientConfig) -> Self {
        Self::new(config, CborCodec)
    }
}

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    pub fn new(config: ClientConfig, codec: C) -> Self {
        ensure_crypto_provider();
        Self {
            http: reqwest::Client::new(),
            config,
            codec,
            state_store: Arc::new(InMemoryStateStore::default()),
            request_authorizer: None,
            schema_sha: None,
        }
    }

    pub fn with_http_client(config: ClientConfig, codec: C, http: reqwest::Client) -> Self {
        Self {
            http,
            config,
            codec,
            state_store: Arc::new(InMemoryStateStore::default()),
            request_authorizer: None,
            schema_sha: None,
        }
    }

    pub fn with_state_store(mut self, state_store: Arc<dyn ClientStateStore>) -> Self {
        self.state_store = state_store;
        self
    }

    pub fn with_optional_state_store(self, state_store: Option<Arc<dyn ClientStateStore>>) -> Self {
        match state_store {
            Some(state_store) => self.with_state_store(state_store),
            None => self,
        }
    }

    pub fn with_request_authorizer(
        mut self,
        request_authorizer: Arc<dyn RequestAuthorizer>,
    ) -> Self {
        self.request_authorizer = Some(request_authorizer);
        self
    }

    /// Stamps the generating schema's `SCHEMA_SHA256` onto this client, so
    /// every subsequent request carries `x-cratestack-schema-sha` (issue
    /// #178). Called by the schema-generated `Client::new` wrapper, not
    /// meant to be called directly by schema authors — public because the
    /// generated code lives in a downstream crate.
    pub fn with_schema_sha(mut self, schema_sha: &'static str) -> Self {
        self.schema_sha = Some(schema_sha);
        self
    }

    pub fn state(&self) -> Result<PersistedClientState, ClientError> {
        // Not `.map_err(ClientError::from)`: `ClientError`'s only
        // `From<CratestackError>` impl targets `ClientError::Codec` (for genuine
        // wire-codec failures), which would misclassify a purely local
        // state-store failure as a remote/codec error — see #475's review
        // findings and `error.rs`'s `state_store_error_maps_to_client_error_state`.
        self.state_store
            .load()
            .map_err(|error| ClientError::State(error.to_string()))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use cratestack_core::CratestackError;

    use super::*;

    /// A `ClientStateStore` whose every operation fails, so tests can
    /// observe how a local state-store failure gets classified without
    /// touching the filesystem or any other real backend.
    #[derive(Debug, Default)]
    pub(crate) struct FailingStateStore;

    impl ClientStateStore for FailingStateStore {
        fn load(&self) -> Result<PersistedClientState, CratestackError> {
            Err(CratestackError::Internal(
                "simulated state store failure".to_owned(),
            ))
        }

        fn save(&self, _state: &PersistedClientState) -> Result<(), CratestackError> {
            Err(CratestackError::Internal(
                "simulated state store failure".to_owned(),
            ))
        }
    }

    /// Regression test for #475's review findings: a `CratestackError` raised by
    /// the state store must surface as `ClientError::State`, not get
    /// silently reclassified as `ClientError::Codec` via the blanket
    /// `From<CratestackError>` impl (which is meant for genuine wire-codec
    /// failures, not local storage failures). Fails against the code that
    /// used `.map_err(ClientError::from)` here.
    #[test]
    fn state_store_error_maps_to_client_error_state() {
        let client = CratestackClient::cbor(ClientConfig::new(
            "http://example.invalid".parse().expect("valid url"),
        ))
        .with_state_store(Arc::new(FailingStateStore));

        let error = client.state().expect_err("state store is rigged to fail");

        match error {
            ClientError::State(message) => {
                assert!(message.contains("simulated state store failure"));
            }
            other => panic!("expected ClientError::State for a state-store failure, got {other:?}"),
        }
    }
}
