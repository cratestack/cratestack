use std::sync::Arc;

use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use reqwest::Url;

use crate::client::CratestackClient;
use crate::config::ClientConfig;
use crate::error::ClientError;
use crate::runtime::transport::RuntimeTransportClient;
use crate::runtime::wire::{
    RuntimeCodecConfig, RuntimeConfigWire, RuntimeEnvelopeConfig, RuntimeErrorCode,
    RuntimeErrorWire, RuntimeRequestWire, RuntimeResponseWire, RuntimeStateStoreConfig,
};
use crate::state::{
    ClientStateStore, InMemoryStateStore, JsonFileStateStore, PersistedClientState,
};
use crate::streaming_callback::RuntimeChunkWire;

pub struct RuntimeHandle {
    runtime: tokio::runtime::Runtime,
    pub(crate) client: RuntimeTransportClient,
}

impl RuntimeHandle {
    pub fn new(config: RuntimeConfigWire) -> Result<Self, RuntimeErrorWire> {
        let base_url = Url::parse(&config.base_url).map_err(|error| RuntimeErrorWire {
            code: RuntimeErrorCode::BadInput,
            http_status: None,
            message: format!("invalid base URL '{}': {error}", config.base_url),
            remote_code: None,
            remote_body: None,
        })?;
        let state_store: Arc<dyn ClientStateStore> = match config.state_store {
            RuntimeStateStoreConfig::InMemory => Arc::new(InMemoryStateStore::default()),
            RuntimeStateStoreConfig::JsonFile { path } => Arc::new(JsonFileStateStore::new(path)),
        };
        if config.transport.envelope != RuntimeEnvelopeConfig::None {
            return Err(RuntimeErrorWire {
                code: RuntimeErrorCode::BadInput,
                http_status: None,
                message: "COSE envelope support is not implemented yet".to_owned(),
                remote_code: None,
                remote_body: None,
            });
        }
        let client = match config.transport.codec {
            RuntimeCodecConfig::Cbor => RuntimeTransportClient::Cbor(
                CratestackClient::new(ClientConfig::new(base_url.clone()), CborCodec)
                    .with_state_store(state_store.clone()),
            ),
            RuntimeCodecConfig::Json => RuntimeTransportClient::Json(
                CratestackClient::new(ClientConfig::new(base_url), JsonCodec)
                    .with_state_store(state_store),
            ),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| RuntimeErrorWire {
                code: RuntimeErrorCode::State,
                http_status: None,
                message: format!("failed to build runtime: {error}"),
                remote_code: None,
                remote_body: None,
            })?;

        Ok(Self { runtime, client })
    }

    pub fn execute(
        &self,
        request: RuntimeRequestWire,
    ) -> Result<RuntimeResponseWire, RuntimeErrorWire> {
        self.runtime
            .block_on(self.client.execute_raw(request))
            .map_err(RuntimeErrorWire::from)
    }

    /// Streaming companion to [`Self::execute`]. The callback receives
    /// one [`RuntimeChunkWire`] per complete cbor-seq item as bytes
    /// arrive on the wire; returning `false` cancels the stream.
    /// Returns when the stream terminates (clean end, error, or
    /// cancellation).
    ///
    /// Designed for the FFI surface — the callback gets raw CBOR bytes
    /// per item, so the host language decodes with its native CBOR
    /// library (Dart, Swift, Kotlin) rather than carrying a typed
    /// generic across the bridge.
    pub fn execute_streamed<F>(
        &self,
        request: RuntimeRequestWire,
        on_chunk: F,
    ) -> Result<(), RuntimeErrorWire>
    where
        F: FnMut(RuntimeChunkWire) -> bool + Send,
    {
        self.runtime
            .block_on(self.client.execute_streamed(request, on_chunk))
    }

    pub fn state(&self) -> Result<PersistedClientState, ClientError> {
        self.client.state()
    }
}
