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

## See Also

- `cratestack-client-rust` — underlying runtime
- `cratestack-client-dart` — generated Dart surface
- [Offline-First with SQLite](https://cratestack.dev/guides/offline-first-sqlite)
- [Client Runtime](https://cratestack.dev/architecture/client-runtime)

## License

MIT
