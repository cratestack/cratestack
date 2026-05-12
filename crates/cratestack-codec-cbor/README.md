# cratestack-codec-cbor

CBOR codec for CrateStack HTTP transport.

## Overview

`cratestack-codec-cbor` is a single-type crate exposing `CborCodec`, a zero-state implementation of the `CoolCodec` trait built on `minicbor-serde`.

## Installation

```toml
[dependencies]
cratestack-codec-cbor = "0.2.2"
```

## Usage

```rust
use cratestack_codec_cbor::CborCodec;
use cratestack_core::CoolCodec;

let codec = CborCodec;
let bytes = codec.encode(&("cool", "stack"))?;
let value: (String, String) = codec.decode(&bytes)?;

assert_eq!(CborCodec::CONTENT_TYPE, "application/cbor");
```

### With generated routes

```rust
let router = cratestack_schema::axum::model_router(cool, CborCodec, AppAuthProvider);
```

### With the Rust client

```rust
use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};

let base_url = url::Url::parse("https://api.example.com")?;
let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
```

## Notes

`minicbor-serde` reports `is_human_readable() = true`, which keeps wire compatibility for types whose serde implementations branch on that hint (uuid, chrono). The macro-emitted projection strips `Value::Null` map entries before reaching this codec, so the non-RFC-8949 "Null = empty array" quirk of this backend never lands on the wire.

The `application/cbor-seq` framing is reserved for streaming responses (`CBOR_SEQUENCE_CONTENT_TYPE` in `cratestack-axum`), but the codec itself does not implement a sequence decoder — generated routers currently emit single-item responses only.

## See Also

- [Transport Architecture](https://cratestack.dev/architecture/transport-architecture)
- `cratestack-codec-json` — JSON codec

## License

MIT
