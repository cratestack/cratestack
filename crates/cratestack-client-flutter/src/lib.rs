//! Flutter-facing wrapper around `cratestack-client-rust`.
//!
//! Exposes Dart-ergonomic wire types, a `FlutterRuntime` handle, a
//! standalone `FlutterCborSeqDecoder` for apps that drive HTTP from Dart,
//! and (cratestack#563) a native CBOR<->JSON bridge in [`mod@cbor`] — the
//! source this crate's `flutter_rust_bridge_codegen` glue is generated
//! from for the published `cratestack_cbor` pub.dev package.

pub mod cbor;
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
