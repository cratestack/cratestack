# cratestack-client-rust

Rust HTTP client runtime for CrateStack services.

## Overview

`cratestack-client-rust` provides a typed HTTP client for calling CrateStack services. Use `include_schema!` or `include_client_macro!` to generate the client surface.

## Installation

```toml
[dependencies]
cratestack-client-rust = "0.2"
tokio = { version = "1", features = ["rt-multi-thread"] }
```

## Usage

### With include_schema!

```rust
use cratestack::include_schema;
use cratestack_client_rust::{CratestackClient, ClientConfig, CborCodec};

include_schema!("../schemas/api.cstack");

// Create client
let runtime = CratestackClient::new(
    ClientConfig::new("https://api.example.com"),
    CborCodec,
);

// Generated client
let client = cratestack_schema::client::Client::new(runtime);

// CRUD operations
let users = client.users().list(&[("limit", "10")], &[]).await?;

let user = client.users().get_view(
    &user_id,
    &User::select()
        .id()
        .email()
        .include_posts(Post::include_selection().id().title()),
    &[],
).await?;

let created = client.users().create(&CreateUserInput {
    email: "user@example.com".to_owned(),
    name: "Alice".to_owned(),
}, &[]).await?;
```

### With include_client_macro!

For standalone client packages:

```rust
use cratestack::include_client_macro;
use cratestack_client_rust::{CratestackClient, ClientConfig, CborCodec};

include_client_macro!("../schemas/api.cstack");

let client = cratestack_schema::client::Client::new(
    CratestackClient::new(
        ClientConfig::new("https://api.example.com"),
        CborCodec,
    )
);
```

## Codecs

```rust
use cratestack_client_rust::{CborCodec, JsonCodec};

// CBOR (recommended for production)
let client = CratestackClient::new(config, CborCodec);

// JSON (for development/interop)
let client = CratestackClient::new(config, JsonCodec);
```

## Request Authorization

Sign requests with canonical request strings:

```rust
use cratestack_client_rust::{RequestAuthorizer, AuthorizationRequest, ClientError};
use std::sync::Arc;

struct HmacAuthorizer {
    key: Vec<u8>,
}

impl RequestAuthorizer for HmacAuthorizer {
    fn authorize(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError> {
        let sig = hmac_sha256(&self.key, request.canonical_request);
        Ok(vec![(
            "authorization".to_owned(),
            format!("Signature {}", hex::encode(sig)),
        )])
    }
}

let client = client.with_request_authorizer(Arc::new(HmacAuthorizer::new(key)));
```

## State Persistence

Journal requests for offline retry:

```rust
use cratestack_client_rust::{JsonFileStateStore, ClientStateStore};
use std::sync::Arc;

let store = Arc::new(JsonFileStateStore::new("./client_state.json"));
let client = client.with_state_store(store);

// Requests are journaled; you can replay after offline recovery
```

## Custom Endpoints

```rust
// Direct HTTP calls
let response: MyType = client
    .get("/custom/endpoint", &[("filter", "active")], &[])
    .await?;

let response: MyType = client
    .post("/custom/endpoint", &input, &[("x-custom", "value")])
    .await?;
```

## Projections

```rust
// Select specific fields
let user = client.users().get_view(
    &user_id,
    &User::select()
        .id()
        .email()
        .include_profile(Profile::include_selection().nickname()),
    &[],
).await?;
```

## See Also

- [Transport Architecture](https://cratestack.dev/architecture/transport-architecture)
- `cratestack-codec-cbor` - CBOR codec
- `cratestack-codec-json` - JSON codec

## License

MIT