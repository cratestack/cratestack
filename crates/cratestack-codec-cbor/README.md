# cratestack-codec-cbor

CBOR codec for CrateStack HTTP transport.

## Overview

`cratestack-codec-cbor` is a single-type crate exposing `CborCodec`, a zero-state implementation of the `CratestackCodec` trait built on `minicbor-serde`.

## Installation

```toml
[dependencies]
cratestack-codec-cbor = "0.7"
```

## Usage

```rust
use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;

let codec = CborCodec;
let bytes = codec.encode(&("cool", "stack"))?;
let value: (String, String) = codec.decode(&bytes)?;

assert_eq!(CborCodec::CONTENT_TYPE, "application/cbor");
```

### With generated routes

```rust
let router = cratestack_schema::axum::model_router(db, CborCodec, AppAuthProvider);
```

### With the Rust client

```rust
use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};

let base_url = url::Url::parse("https://api.example.com")?;
let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
```

## Notes

`minicbor-serde` reports `is_human_readable() == false`, so types whose serde implementations branch on that hint (uuid, chrono, `cratestack_core::Value`'s `Bytes` arm) take their binary branch under this codec. The macro-emitted projection (`cratestack-axum`'s `ProjectedValue`) gives `Null` its own variant that always calls `serialize_none()`, matching `Option::<T>::None`'s own encoding — the non-RFC-8949 "Null = empty array" quirk this backend has for `serialize_unit()` never lands on the wire because nothing routes a null through that path.

The `application/cbor-seq` framing (`CBOR_SEQUENCE_CONTENT_TYPE` in `cratestack-axum`) is used for `@stream` procedure responses — generated routers emit genuinely incremental cbor-seq framing for those today. This crate's own `CborCodec`, though, only implements single-item encode/decode; the sequence framing lives in `cratestack-axum` instead.

## See Also

- [Transport Architecture](https://cratestack.dev/architecture/transport-architecture)
- `cratestack-codec-json` — JSON codec

## License

MIT
