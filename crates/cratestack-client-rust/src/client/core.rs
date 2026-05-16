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
        }
    }

    pub fn with_http_client(config: ClientConfig, codec: C, http: reqwest::Client) -> Self {
        Self {
            http,
            config,
            codec,
            state_store: Arc::new(InMemoryStateStore::default()),
            request_authorizer: None,
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

    pub fn state(&self) -> Result<PersistedClientState, ClientError> {
        self.state_store.load()
    }
}
