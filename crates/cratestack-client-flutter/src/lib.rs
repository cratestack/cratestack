use std::path::PathBuf;
use std::sync::Mutex;

use cratestack_client_rust::{
    ClientError, RuntimeChunkWire, RuntimeCodecConfig, RuntimeConfigWire, RuntimeEnvelopeConfig,
    RuntimeErrorCode, RuntimeErrorWire, RuntimeHandle, RuntimeHeader, RuntimeRequestWire,
    RuntimeResponseWire, RuntimeStateStoreConfig, RuntimeTransportConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlutterHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FlutterStateStoreConfig {
    InMemory,
    JsonFile { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlutterRuntimeConfig {
    pub base_url: String,
    pub state_store: FlutterStateStoreConfig,
    pub transport: FlutterRuntimeTransportConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum FlutterRuntimeCodec {
    #[default]
    Cbor,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum FlutterRuntimeEnvelope {
    #[default]
    None,
    CoseSign1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FlutterRuntimeTransportConfig {
    pub codec: FlutterRuntimeCodec,
    pub envelope: FlutterRuntimeEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlutterRequest {
    pub method: String,
    pub path: String,
    pub canonical_query: Option<String>,
    pub headers: Vec<FlutterHeader>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlutterResponse {
    pub status_code: u16,
    pub headers: Vec<FlutterHeader>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlutterRequestJournalEntry {
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub content_type: Option<String>,
    pub recorded_at_rfc3339: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlutterPersistedState {
    pub schema_version: u32,
    pub state_version: u64,
    pub request_journal: Vec<FlutterRequestJournalEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlutterRuntimeError {
    pub code: u32,
    pub http_status: Option<u16>,
    pub message: String,
    pub remote_code: Option<String>,
    pub remote_body: Option<Vec<u8>>,
}

/// Streaming-response chunk shape exposed to Flutter.
///
/// The native Rust side of a flutter_rust_bridge app exposes this enum
/// to Dart by wrapping `FlutterRuntime::execute_streamed` with a
/// `StreamSink<FlutterChunkWire>` argument. From Dart, the app code
/// gets a `Stream<FlutterChunkWire>` that yields one variant per
/// complete cbor-seq item over the wire:
///
/// - `Item(Vec<u8>)` — one CBOR-encoded item. Decode it on the Dart
///   side with the `cbor` package; the bytes are exactly what the
///   server emitted.
/// - `End` — the server closed the stream cleanly. No further chunks.
/// - `Error(FlutterRuntimeError)` — the stream failed mid-flight. No
///   further chunks. Same shape as `FlutterRuntimeError` from `execute`,
///   so the Dart error handling reuses one match arm.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FlutterChunkWire {
    Item(Vec<u8>),
    End,
    Error(FlutterRuntimeError),
}

pub struct FlutterRuntime {
    inner: Mutex<RuntimeHandle>,
}

impl FlutterRuntime {
    pub fn new(config: FlutterRuntimeConfig) -> Result<Self, FlutterRuntimeError> {
        let handle = RuntimeHandle::new(config.into()).map_err(FlutterRuntimeError::from)?;
        Ok(Self {
            inner: Mutex::new(handle),
        })
    }

    pub fn execute(&self, request: FlutterRequest) -> Result<FlutterResponse, FlutterRuntimeError> {
        let handle = self.inner.lock().map_err(|error| FlutterRuntimeError {
            code: RuntimeErrorCode::State as u32,
            http_status: None,
            message: format!("failed to lock runtime handle: {error}"),
            remote_code: None,
            remote_body: None,
        })?;
        handle
            .execute(request.into())
            .map(FlutterResponse::from)
            .map_err(FlutterRuntimeError::from)
    }

    /// POST /rpc/{op_id} — unary call.
    ///
    /// `op_id` is the dotted dispatch key the server emits (e.g.
    /// `model.User.list`, `procedure.publishPost`). `input_json` is the
    /// JSON-encoded RPC input body. Returns the full `FlutterResponse`
    /// so the Dart side can decode the body against the right type.
    pub fn rpc_call(
        &self,
        op_id: &str,
        input_json: Vec<u8>,
        headers: Vec<FlutterHeader>,
    ) -> Result<FlutterResponse, FlutterRuntimeError> {
        let request = FlutterRequest {
            method: "POST".to_owned(),
            path: format!("/rpc/{}", op_id),
            canonical_query: None,
            headers,
            body: input_json,
        };
        self.execute(request)
    }

    /// POST /rpc/batch — batched call.
    pub fn rpc_batch(
        &self,
        batch_json: Vec<u8>,
        headers: Vec<FlutterHeader>,
    ) -> Result<FlutterResponse, FlutterRuntimeError> {
        let request = FlutterRequest {
            method: "POST".to_owned(),
            path: "/rpc/batch".to_owned(),
            canonical_query: None,
            headers,
            body: batch_json,
        };
        self.execute(request)
    }

    /// Streaming companion to [`Self::rpc_call`]. POSTs to
    /// `/rpc/{op_id}` with `Accept: application/cbor-seq` and delivers
    /// one [`FlutterChunkWire`] per item as bytes arrive on the wire;
    /// returning `false` from the callback cancels the stream.
    ///
    /// `op_id` is the dotted dispatch key the server emits — typically
    /// `model.X.list` for sequence-returning CRUD or `procedure.<name>`
    /// for list-return procedures. `input` is the codec-encoded RPC
    /// input body; decode the per-item bytes on the Dart side against
    /// the `Output` type the op produces.
    ///
    /// Wrap this with a `flutter_rust_bridge` `StreamSink<FlutterChunkWire>`
    /// in the consuming Flutter app — same pattern as
    /// [`Self::execute_streamed`].
    pub fn rpc_call_streamed<F>(
        &self,
        op_id: &str,
        input: Vec<u8>,
        headers: Vec<FlutterHeader>,
        on_chunk: F,
    ) -> Result<(), FlutterRuntimeError>
    where
        F: FnMut(FlutterChunkWire) -> bool + Send,
    {
        let request = FlutterRequest {
            method: "POST".to_owned(),
            path: format!("/rpc/{}", op_id),
            canonical_query: None,
            headers,
            body: input,
        };
        self.execute_streamed(request, on_chunk)
    }

    /// Streaming companion to [`Self::execute`]. The callback receives
    /// one [`FlutterChunkWire`] per complete cbor-seq item as bytes
    /// arrive on the wire; returning `false` cancels the stream.
    /// Returns when the stream terminates (clean end, error, or
    /// cancellation).
    ///
    /// Designed to be wrapped by `flutter_rust_bridge`'s
    /// `StreamSink<FlutterChunkWire>` in the consuming Flutter app —
    /// see the example in `examples/embedded-flutter/native` for the
    /// thin Dart-callable wrapper pattern.
    pub fn execute_streamed<F>(
        &self,
        request: FlutterRequest,
        mut on_chunk: F,
    ) -> Result<(), FlutterRuntimeError>
    where
        F: FnMut(FlutterChunkWire) -> bool + Send,
    {
        let handle = self.inner.lock().map_err(|error| FlutterRuntimeError {
            code: RuntimeErrorCode::State as u32,
            http_status: None,
            message: format!("failed to lock runtime handle: {error}"),
            remote_code: None,
            remote_body: None,
        })?;
        handle
            .execute_streamed(request.into(), move |chunk| {
                on_chunk(FlutterChunkWire::from(chunk))
            })
            .map_err(FlutterRuntimeError::from)
    }
}

impl From<RuntimeChunkWire> for FlutterChunkWire {
    fn from(value: RuntimeChunkWire) -> Self {
        match value {
            RuntimeChunkWire::Item(bytes) => Self::Item(bytes),
            RuntimeChunkWire::End => Self::End,
            RuntimeChunkWire::Error(error) => Self::Error(FlutterRuntimeError::from(error)),
        }
    }
}

impl From<FlutterRuntimeConfig> for RuntimeConfigWire {
    fn from(value: FlutterRuntimeConfig) -> Self {
        Self {
            base_url: value.base_url,
            state_store: match value.state_store {
                FlutterStateStoreConfig::InMemory => RuntimeStateStoreConfig::InMemory,
                FlutterStateStoreConfig::JsonFile { path } => RuntimeStateStoreConfig::JsonFile {
                    path: PathBuf::from(path),
                },
            },
            transport: RuntimeTransportConfig {
                codec: match value.transport.codec {
                    FlutterRuntimeCodec::Cbor => RuntimeCodecConfig::Cbor,
                    FlutterRuntimeCodec::Json => RuntimeCodecConfig::Json,
                },
                envelope: match value.transport.envelope {
                    FlutterRuntimeEnvelope::None => RuntimeEnvelopeConfig::None,
                    FlutterRuntimeEnvelope::CoseSign1 => RuntimeEnvelopeConfig::CoseSign1,
                },
            },
        }
    }
}

impl From<FlutterHeader> for RuntimeHeader {
    fn from(value: FlutterHeader) -> Self {
        Self {
            name: value.name,
            value: value.value,
        }
    }
}

impl From<RuntimeHeader> for FlutterHeader {
    fn from(value: RuntimeHeader) -> Self {
        Self {
            name: value.name,
            value: value.value,
        }
    }
}

impl From<FlutterRequest> for RuntimeRequestWire {
    fn from(value: FlutterRequest) -> Self {
        Self {
            method: value.method,
            path: value.path,
            canonical_query: value.canonical_query,
            headers: value.headers.into_iter().map(RuntimeHeader::from).collect(),
            body: value.body,
        }
    }
}

impl From<RuntimeResponseWire> for FlutterResponse {
    fn from(value: RuntimeResponseWire) -> Self {
        Self {
            status_code: value.status_code,
            headers: value.headers.into_iter().map(FlutterHeader::from).collect(),
            body: value.body,
        }
    }
}

impl From<cratestack_client_rust::PersistedClientState> for FlutterPersistedState {
    fn from(value: cratestack_client_rust::PersistedClientState) -> Self {
        Self {
            schema_version: value.schema_version,
            state_version: value.state_version,
            request_journal: value
                .request_journal
                .into_iter()
                .map(|entry| FlutterRequestJournalEntry {
                    method: entry.method,
                    path: entry.path,
                    status_code: entry.status_code,
                    content_type: entry.content_type,
                    recorded_at_rfc3339: entry.recorded_at.to_rfc3339(),
                })
                .collect(),
        }
    }
}

impl From<RuntimeErrorWire> for FlutterRuntimeError {
    fn from(value: RuntimeErrorWire) -> Self {
        Self {
            code: value.code as u32,
            http_status: value.http_status,
            message: value.message,
            remote_code: value.remote_code,
            remote_body: value.remote_body,
        }
    }
}

impl From<ClientError> for FlutterRuntimeError {
    fn from(value: ClientError) -> Self {
        match value {
            ClientError::Transport(error) => Self {
                code: RuntimeErrorCode::Transport as u32,
                http_status: None,
                message: error.to_string(),
                remote_code: None,
                remote_body: None,
            },
            ClientError::Codec(error) => Self {
                code: RuntimeErrorCode::Codec as u32,
                http_status: Some(error.status_code().as_u16()),
                message: error.to_string(),
                remote_code: Some(error.code().to_owned()),
                remote_body: None,
            },
            ClientError::State(message) => Self {
                code: RuntimeErrorCode::State as u32,
                http_status: None,
                message,
                remote_code: None,
                remote_body: None,
            },
            ClientError::InvalidResponse(message) => Self {
                code: RuntimeErrorCode::InvalidResponse as u32,
                http_status: None,
                message,
                remote_code: None,
                remote_body: None,
            },
            ClientError::BadInput(message) => Self {
                code: RuntimeErrorCode::BadInput as u32,
                http_status: None,
                message,
                remote_code: None,
                remote_body: None,
            },
            ClientError::Remote {
                status,
                error,
                message,
            } => Self {
                code: RuntimeErrorCode::Remote as u32,
                http_status: Some(status.as_u16()),
                remote_code: error.as_ref().map(|value| value.code.clone()),
                remote_body: error
                    .as_ref()
                    .and_then(|value| serde_json::to_vec(value).ok()),
                message,
            },
        }
    }
}
