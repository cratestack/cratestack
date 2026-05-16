//! Wire-level value types exposed to Flutter.

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
