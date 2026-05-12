# cratestack-codec-json

JSON codec for CrateStack HTTP transport.

## Overview

`cratestack-codec-json` exposes `JsonCodec`, a zero-state implementation of the `CoolCodec` trait built on `serde_json`. It is the right choice for human-readable interop and browser clients that do not negotiate CBOR.

## Installation

```toml
[dependencies]
cratestack-codec-json = "0.2.2"
```

## Usage

```rust
use cratestack_codec_json::JsonCodec;
use cratestack_core::CoolCodec;

let codec = JsonCodec;
let bytes = codec.encode(&("cool", "stack"))?;
let value: (String, String) = codec.decode(&bytes)?;

assert_eq!(JsonCodec::CONTENT_TYPE, "application/json");
```

### With generated routes

```rust
let router = cratestack_schema::axum::model_router(cool, JsonCodec, AppAuthProvider);
```

### With the Rust client

```rust
use cratestack_client_rust::{ClientConfig, CratestackClient, JsonCodec};

let base_url = url::Url::parse("https://api.example.com")?;
let client = CratestackClient::new(ClientConfig::new(base_url), JsonCodec);
```

## See Also

- [Transport Architecture](https://cratestack.dev/architecture/transport-architecture)
- `cratestack-codec-cbor` — CBOR codec, preferred for production internal traffic

## License

MIT
