//! Flutter-facing wrapper around `cratestack-client-rust`.
//!
//! Exposes Dart-ergonomic wire types, a `FlutterRuntime` handle, a
//! standalone `FlutterCborSeqDecoder` for apps that drive HTTP from Dart,
//! and (cratestack#563) a native CBOR<->JSON bridge in [`mod@cbor`] — the
//! source this crate's `flutter_rust_bridge_codegen` glue is generated
//! from for the published `cratestack_cbor` pub.dev package.
//!
//! `mod frb_generated` is the Rust half of that generated glue
//! (`flutter_rust_bridge_codegen generate`'s `rust_output`, see
//! `flutter_rust_bridge.yaml`). It is gitignored, not committed
//! (cratestack#563 decision — generated in CI/locally via `just
//! frb-generate crates/cratestack-client-flutter`, never checked in), and
//! gated behind the `frb-glue` Cargo feature (off by default) so a fresh
//! checkout's default `cargo build`/`cargo test -p cratestack-client-flutter`
//! never needs the generated file to exist. Only building with `--features
//! frb-glue` does — that's what regenerating the glue and then compiling
//! against it (`just frb-generate` + `cargo build -p cratestack-client-flutter
//! --features frb-glue`) is for.
#[cfg(feature = "frb-glue")]
mod frb_generated;

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
