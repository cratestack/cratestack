## 0.8.0

- Initial package structure (cratestack#563). One uniform `CratestackCborCodec`
  API, auto-selected per platform:
  - Native: flutter_rust_bridge over a vendored prebuilt library. This
    release vendors **Linux x86_64 only** — a deliberate one-platform
    spike proving the vendoring pattern before the full platform matrix.
  - Web: the existing `cratestack-cbor-wasm` wasm-bindgen artifact,
    vendored and loaded via `dart:js_interop`.
- Not yet published to pub.dev — see README.md.
