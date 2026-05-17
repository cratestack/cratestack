# cratestack-client-rust

Rust HTTP client runtime for CrateStack services.

## Overview

`cratestack-client-rust` provides the typed client runtime that `include_client_schema!` builds its generated `client::Client` surface on top of. It owns the HTTP transport, codec negotiation, request authorization hook, and optional offline state journaling.

The CBOR and JSON codecs are re-exported as `CborCodec` and `JsonCodec`.

## Installation

```toml
[dependencies]
cratestack-client-rust = "0.2.2"
tokio = { version = "1", features = ["rt-multi-thread"] }
url = "2"
```

## Usage

```rust
use cratestack::include_client_schema;
use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};

include_client_schema!("../schemas/api.cstack");

let base_url = url::Url::parse("https://api.example.com")?;
let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
let client = cratestack_schema::client::Client::new(runtime);
```

## Codecs

```rust
use cratestack_client_rust::{CborCodec, JsonCodec};

let cbor_client = CratestackClient::new(config.clone(), CborCodec);
let json_client = CratestackClient::new(config, JsonCodec);
```

## Request Authorization

`with_request_authorizer` attaches an implementation of `RequestAuthorizer` that returns extra headers per call. The trait gets a canonical-request string the implementer can sign:

```rust
use std::sync::Arc;
use cratestack_client_rust::{AuthorizationRequest, ClientError, RequestAuthorizer};

struct HmacAuthorizer { key: Vec<u8> }

impl RequestAuthorizer for HmacAuthorizer {
    fn authorize(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError> {
        let sig = sign(&self.key, &request.canonical_request_string());
        Ok(vec![(
            "authorization".to_owned(),
            format!("Signature {}", hex::encode(sig)),
        )])
    }
}

let client = runtime.with_request_authorizer(Arc::new(HmacAuthorizer { key }));
```

## State Persistence

Journal requests for replay or offline recovery. The bundled implementations are `InMemoryStateStore` and `JsonFileStateStore`; the trait is `ClientStateStore`.

```rust
use std::sync::Arc;
use cratestack_client_rust::{ClientStateStore, JsonFileStateStore};

let store: Arc<dyn ClientStateStore> = Arc::new(JsonFileStateStore::new("./client_state.json"));
let runtime = runtime.with_state_store(store);
```

`with_optional_state_store(None)` is a no-op convenience for configuration-driven setup.

For a Redis-backed store, see `cratestack-client-store-redis`. For a SQLite-backed store, see `cratestack-client-store-sqlite`.

## See Also

- [Client Runtime](https://cratestack.dev/architecture/client-runtime)
- [Transport Architecture](https://cratestack.dev/architecture/transport-architecture)
- `cratestack-codec-cbor` — CBOR codec
- `cratestack-codec-json` — JSON codec
- `cratestack-client-store-redis` — Redis-backed `ClientStateStore`
- `cratestack-client-store-sqlite` — SQLite-backed `ClientStateStore`

## License

MIT
