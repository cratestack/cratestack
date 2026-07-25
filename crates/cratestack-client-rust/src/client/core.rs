use std::sync::Arc;

use cratestack_codec_cbor::CborCodec;

use crate::auth::RequestAuthorizer;
use crate::codec::HttpClientCodec;
use crate::config::ClientConfig;
use crate::error::ClientError;
use crate::state::{ClientStateStore, InMemoryStateStore, PersistedClientState};

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
        self.state_store.load()
    }
}
