use std::sync::Arc;

use cratestack_codec_cbor::CborCodec;
#[cfg(feature = "codec-json")]
use cratestack_codec_json::JsonCodec;
use reqwest::Url;

use crate::client::CratestackClient;
use crate::config::ClientConfig;
use crate::error::ClientError;
use crate::runtime::sealing::{Sealing, bad_input};
use crate::runtime::transport::RuntimeTransportClient;
use crate::runtime::wire::{
    RuntimeCodecConfig, RuntimeConfigWire, RuntimeErrorCode, RuntimeErrorWire, RuntimeRequestWire,
    RuntimeResponseWire, RuntimeStateStoreConfig,
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
    /// A handle with no envelope. A config that names one
    /// (`CoseSign1`, `CoseMac0`) is refused: the keys cannot travel in the
    /// config, so build the handle with `RuntimeHandle::with_envelope` (feature `cose`).
    pub fn new(config: RuntimeConfigWire) -> Result<Self, RuntimeErrorWire> {
        Self::build(config, Sealing::None)
    }

    /// A handle that seals every request and opens every response
    /// (cratestack#1007). `config.transport.envelope` must name `envelope`'s
    /// mode (`CoseSign1` for Sign1, `CoseMac0` for Mac0) and its codec must
    /// be CBOR. `schema_sha` is the generated client schema's
    /// `SCHEMA_SHA256_BYTES`: the raw bridge has no generated `Client` to
    /// stamp it, and every binding needs it.
    ///
    /// Raw requests must be RPC ones (`/rpc/{op_id}`, `/rpc/batch`); a raw
    /// REST path has no route template to bind. Streams are refused
    /// (`envelope_streams_unsupported`).
    #[cfg(feature = "cose")]
    pub fn with_envelope(
        config: RuntimeConfigWire,
        envelope: crate::envelope::ClientEnvelope,
        schema_sha: &'static [u8; 32],
    ) -> Result<Self, RuntimeErrorWire> {
        Self::build(config, Sealing::Cose(envelope, schema_sha))
    }

    fn build(config: RuntimeConfigWire, sealing: Sealing) -> Result<Self, RuntimeErrorWire> {
        let base_url = Url::parse(&config.base_url).map_err(|error| {
            bad_input(format!("invalid base URL '{}': {error}", config.base_url))
        })?;
        let state_store: Arc<dyn ClientStateStore> = match config.state_store {
            RuntimeStateStoreConfig::InMemory => Arc::new(InMemoryStateStore::default()),
            RuntimeStateStoreConfig::JsonFile { path } => Arc::new(JsonFileStateStore::new(path)),
        };
        sealing.agrees_with(&config.transport.envelope)?;
        let client = match config.transport.codec {
            RuntimeCodecConfig::Cbor => RuntimeTransportClient::Cbor(
                sealing.apply(
                    CratestackClient::new(ClientConfig::new(base_url.clone()), CborCodec)
                        .with_state_store(state_store.clone()),
                )?,
            ),
            #[cfg(feature = "codec-json")]
            RuntimeCodecConfig::Json => RuntimeTransportClient::Json(
                sealing.apply(
                    CratestackClient::new(ClientConfig::new(base_url), JsonCodec)
                        .with_state_store(state_store),
                )?,
            ),
            #[cfg(not(feature = "codec-json"))]
            RuntimeCodecConfig::Json => {
                let _ = state_store;
                return Err(bad_input(
                    "JSON codec is not compiled in (cratestack-client-rust built with \
                     `--no-default-features` or without the `codec-json` feature)"
                        .to_owned(),
                ));
            }
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
