//! Flutter-facing wrapper around `cratestack-client-rust`.
//!
//! Exposes Dart-ergonomic wire types, a `FlutterRuntime` handle, and a
//! standalone `FlutterCborSeqDecoder` for apps that drive HTTP from Dart.

mod conversions;
mod decoder;
mod runtime;
mod types;

pub use decoder::FlutterCborSeqDecoder;
pub use runtime::FlutterRuntime;
pub use types::{
    FlutterChunkWire, FlutterHeader, FlutterPersistedState, FlutterRequest,
    FlutterRequestJournalEntry, FlutterResponse, FlutterRuntimeCodec, FlutterRuntimeConfig,
    FlutterRuntimeEnvelope, FlutterRuntimeError, FlutterRuntimeTransportConfig,
    FlutterStateStoreConfig,
};
