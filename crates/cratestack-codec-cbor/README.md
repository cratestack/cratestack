# cratestack-codec-cbor

CBOR codec implementation for CrateStack HTTP transport.

## Overview

`cratestack-codec-cbor` implements the `CoolCodec` trait for CBOR (Concise Binary Object Representation) encoding/decoding.

## Installation

```toml
[dependencies]
cratestack-codec-cbor = "0.2"
```

## Usage

```rust
use cratestack_codec_cbor::CborCodec;
use cratestack_core::CoolCodec;

let codec = CborCodec;

// Encode
let bytes = codec.encode(&my_struct)?;

// Decode
let value: MyStruct = codec.decode(&bytes)?;

// Content type
assert_eq!(CborCodec::CONTENT_TYPE, "application/cbor");
```

## Codec Trait

Implements `CoolCodec` from `cratestack-core`:

```rust
pub trait CoolCodec {
    const CONTENT_TYPE: &'static str;
    fn encode<T: Serialize>(&self, value: &T) -> Result<Vec<u8>, CoolError>;
    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, CoolError>;
}
```

## Transport Integration

Use with generated Axum routes:

```rust
use cratestack_codec_cbor::CborCodec;

let router = cratestack_schema::axum::model_router(
    cool,
    CborCodec,
    AppAuthProvider,
);
```

Or with the Rust client:

```rust
use cratestack_client_rust::{CratestackClient, ClientConfig, CborCodec};

let client = CratestackClient::new(
    ClientConfig::new("https://api.example.com"),
    CborCodec,
);
```

## CBOR Sequence

For streaming responses, use `application/cbor-seq`:

```rust
use cratestack_codec_cbor::CborCodec;

// Encode sequence
let mut bytes = Vec::new();
for item in items {
    bytes.extend(codec.encode(&item)?);
}

// Decode sequence
let items: Vec<Item> = codec.decode_sequence(&bytes)?;
```

## See Also

- [Transport Architecture](https://cratestack.dev/architecture/transport-architecture)
- `cratestack-codec-json` - JSON codec

## License

MIT