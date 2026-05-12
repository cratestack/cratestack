# cratestack-codec-json

JSON codec implementation for CrateStack HTTP transport.

## Overview

`cratestack-codec-json` implements the `CoolCodec` trait for JSON encoding/decoding. Useful for development, debugging, and clients that don't support CBOR.

## Installation

```toml
[dependencies]
cratestack-codec-json = "0.2"
```

## Usage

```rust
use cratestack_codec_json::JsonCodec;
use cratestack_core::CoolCodec;

let codec = JsonCodec;

// Encode
let bytes = codec.encode(&my_struct)?;

// Decode
let value: MyStruct = codec.decode(&bytes)?;

// Content type
assert_eq!(JsonCodec::CONTENT_TYPE, "application/json");
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

Use with generated routes for JSON support:

```rust
use cratestack_codec_json::JsonCodec;

let router = cratestack_schema::axum::model_router(
    cool,
    JsonCodec,
    AppAuthProvider,
);
```

## When to Use

- **Development/Debugging**: JSON is human-readable
- **Interoperability**: Clients without CBOR support
- **Browser Clients**: Direct fetch API compatibility

For production internal services, prefer `CborCodec` for better performance and smaller payloads.

## See Also

- [Transport Architecture](https://cratestack.dev/architecture/transport-architecture)
- `cratestack-codec-cbor` - CBOR codec

## License

MIT