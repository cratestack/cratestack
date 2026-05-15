# cratestack-client-flutter

Flutter bridge runtime for CrateStack clients.

## Overview

`cratestack-client-flutter` is a thin Rust crate that exposes the `cratestack-client-rust` `RuntimeHandle` through `flutter_rust_bridge`-friendly types. It is the Rust side of the architecture documented in [Offline-First with SQLite](https://cratestack.dev/guides/offline-first-sqlite): Rust owns state, persistence, and business logic; Flutter is UI only.

The public surface is purely `FlutterRuntime` plus wire-shaped POD structs (`FlutterRequest`, `FlutterResponse`, `FlutterRuntimeConfig`, `FlutterStateStoreConfig`, `FlutterRuntimeError`, etc.) — no Flutter widgets, no Dart code, and no schema-specific surface. Use this crate from a host-app cdylib that exports the bridge bindings.

## Installation

```toml
[dependencies]
cratestack-client-flutter = "0.2.2"
```

## Usage

```rust
use cratestack_client_flutter::{
    FlutterRuntime, FlutterRuntimeConfig, FlutterRuntimeCodec, FlutterRuntimeEnvelope,
    FlutterRuntimeTransportConfig, FlutterStateStoreConfig, FlutterRequest,
};

let runtime = FlutterRuntime::new(FlutterRuntimeConfig {
    base_url: "https://api.example.com".to_owned(),
    state_store: FlutterStateStoreConfig::JsonFile { path: "/app/state.json".to_owned() },
    transport: FlutterRuntimeTransportConfig {
        codec: FlutterRuntimeCodec::Cbor,
        envelope: FlutterRuntimeEnvelope::None,
    },
})?;

let response = runtime.execute(FlutterRequest {
    method: "GET".to_owned(),
    path: "/api/users".to_owned(),
    canonical_query: None,
    headers: vec![],
    body: vec![],
})?;
```

Codec options are `Cbor` and `Json`. Envelope options are `None` and `CoseSign1` (envelope wiring lives in `cratestack-client-rust`).

## Streaming (`application/cbor-seq`)

For list-return procedures and any `Sequence`-kind RPC op, `FlutterRuntime::execute_streamed` delivers items **as bytes arrive on the wire** instead of buffering the full response body. On a flaky or metered mobile link this drops first-item latency from "buffer the whole body" to "decode one chunk."

The Rust shape is callback-driven; the typical Flutter integration wraps it with a `flutter_rust_bridge` `StreamSink<FlutterChunkWire>` so Dart code sees a normal `Stream<FlutterChunkWire>`:

```rust
// In your Flutter app's native crate (the one running flutter_rust_bridge_codegen):
use cratestack_client_flutter::{
    FlutterChunkWire, FlutterRequest, FlutterRuntime, FlutterRuntimeError,
};
use flutter_rust_bridge::frb;

#[frb(sync)]
pub fn execute_streamed(
    runtime: &FlutterRuntime,
    request: FlutterRequest,
    sink: flutter_rust_bridge::StreamSink<FlutterChunkWire>,
) -> Result<(), FlutterRuntimeError> {
    runtime.execute_streamed(request, move |chunk| {
        // Push to Dart. `add` returns Err if the Dart side cancelled
        // (await-for loop broke out / sink closed) — propagate that as
        // a cancellation signal so the stream stops cleanly.
        sink.add(chunk).is_ok()
    })
}
```

On the Dart side (after `flutter_rust_bridge_codegen generate`):

```dart
final Stream<FlutterChunkWire> stream = executeStreamed(
    runtime: runtime,
    request: request,
);

await for (final chunk in stream) {
    switch (chunk) {
        case FlutterChunkWire_Item(:final field0):
            final item = cbor.decode(field0); // any Dart CBOR package
            renderRow(item);
        case FlutterChunkWire_End():
            break;
        case FlutterChunkWire_Error(:final field0):
            handleError(field0);
            break;
    }
}
```

The chunked decoder (see `cratestack-client-rust::CborSeqChunkDecoder`) drives `reqwest::Response::bytes_stream()` and emits one `Item(Vec<u8>)` per complete CBOR item. Cancellation, terminal-end, and transport errors all flow as variants of `FlutterChunkWire`, so the Dart consumer needs just one match arm to cover all paths.

### RPC streaming

For schemas declared with `transport rpc`, list-return procedures (and any sequence-kind op) are served at `POST /rpc/{op_id}` with the same `application/cbor-seq` framing. `FlutterRuntime::rpc_call_streamed` is the dedicated entrypoint — same callback shape as `execute_streamed`, just constructs the `/rpc/{op_id}` URL for you:

```rust
#[frb(sync)]
pub fn rpc_call_streamed(
    runtime: &FlutterRuntime,
    op_id: String,
    input: Vec<u8>,
    headers: Vec<FlutterHeader>,
    sink: flutter_rust_bridge::StreamSink<FlutterChunkWire>,
) -> Result<(), FlutterRuntimeError> {
    runtime.rpc_call_streamed(&op_id, input, headers, move |chunk| sink.add(chunk).is_ok())
}
```

`op_id` is the dotted dispatch key the server emits — `model.User.list` for sequence-returning CRUD or `procedure.<name>` for list-return procedures. `input` is the codec-encoded RPC input body; decode each `FlutterChunkWire::Item(bytes)` on the Dart side against the right `Output` type for the op.

End-to-end tests live in [`tests/streaming_bridge.rs`](tests/streaming_bridge.rs).

## See Also

- `cratestack-client-rust` — underlying runtime
- `cratestack-client-dart` — generated Dart surface
- [Offline-First with SQLite](https://cratestack.dev/guides/offline-first-sqlite)
- [Client Runtime](https://cratestack.dev/architecture/client-runtime)

## License

MIT
