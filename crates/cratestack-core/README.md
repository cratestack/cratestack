# cratestack-core

Core types, traits, and error handling shared across the CrateStack framework.

## Overview

`cratestack-core` provides the foundational types that all other CrateStack crates depend on:

- **Error handling**: `CoolError` with typed HTTP status codes
- **Auth context**: `CoolContext`, `PrincipalContext`, `AuthProvider`
- **Schema AST**: `Schema`, `Model`, `Field`, `Procedure`, etc.
- **Audit**: `AuditEvent`, `AuditSink`, `AuditOperation`
- **Signed envelope**: `HmacEnvelope` for request authentication
- **Decimal**: Selectable backend (`rust_decimal` or `bigdecimal`)
- **Event bus**: `CoolEventBus` for model lifecycle events

## Installation

```toml
[dependencies]
cratestack-core = "0.2"

[features]
default = ["decimal-rust-decimal"]
```

## Error Handling

`CoolError` provides typed HTTP errors with safe public messages:

```rust
use cratestack_core::CoolError;
use http::StatusCode;

// 4xx - message is returned to clients
let err = CoolError::BadRequest("missing query parameter".to_owned());
assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
assert_eq!(err.public_message(), "missing query parameter");

// 5xx - only canned message returned, detail logged
let err = CoolError::Database("connection refused".to_owned());
assert_eq!(err.public_message(), "internal error"); // safe for clients
let detail = err.detail(); // "connection refused" - operator only
```

## Auth Context

`CoolContext` carries the authenticated principal:

```rust
use cratestack_core::{CoolContext, AuthProvider, RequestContext, Value};
use http::HeaderMap;

// Anonymous context
let ctx = CoolContext::anonymous();

// Authenticated from principal
let ctx = CoolContext::from_principal(serde_json::json!({
    "id": "usr_123",
    "role": "admin",
    "tenant": { "id": "org_456" }
}));

// Access claims
assert_eq!(ctx.auth_field("id"), Some(&Value::String("usr_123".to_owned())));
assert_eq!(ctx.tenant_id(), Some("org_456"));
```

### AuthProvider Trait

Host applications implement `AuthProvider` to resolve auth from HTTP requests:

```rust
use cratestack_core::{AuthProvider, CoolContext, CoolError, RequestContext};

#[derive(Clone)]
struct MyAuthProvider;

impl AuthProvider for MyAuthProvider {
    type Error = CoolError;

    async fn authenticate(&self, request: &RequestContext<'_>) -> Result<CoolContext, Self::Error> {
        // Resolve from headers, cookies, bearer tokens, etc.
        let token = request.headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| CoolError::Unauthorized("missing token".to_owned()))?;
        
        // Validate token and build context
        let principal = validate_token(token)?;
        Ok(CoolContext::from_principal(Some(principal))?)
    }
}
```

## Schema AST

Parsed `.cstack` representation:

```rust
use cratestack_core::Schema;

let schema: Schema = /* parsed from cratestack-parser */;

for model in &schema.models {
    println!("model {}", model.name);
    for field in &model.fields {
        println!("  {}: {:?}", field.name, field.ty);
    }
}
```

## Audit Events

Record model mutations for compliance:

```rust
use cratestack_core::{AuditEvent, AuditOperation, AuditActor};
use chrono::Utc;

let event = AuditEvent {
    event_id: uuid::Uuid::new_v4(),
    schema_name: "banking".to_owned(),
    model: "Account".to_owned(),
    operation: AuditOperation::Update,
    primary_key: serde_json::json!({"id": "acc_123"}),
    actor: AuditActor {
        id: Some("usr_456".to_owned()),
        claims: Default::default(),
        ip: Some("192.168.1.1".to_owned()),
    },
    tenant: None,
    before: Some(serde_json::json!({"balance": 1000})),
    after: Some(serde_json::json!({"balance": 900})),
    request_id: Some("trace-abc".to_owned()),
    occurred_at: Utc::now(),
};
```

## Decimal Backend

Select at compile time:

```toml
[features]
default = ["decimal-rust-decimal"]  # 128-bit fixed precision
# decimal-bigdecimal = ["cratestack-core/decimal-bigdecimal"]  # arbitrary precision
```

```rust
use cratestack_core::Decimal;

let amount: Decimal = "123.45".parse()?;
```

## Transaction Isolation

PostgreSQL isolation levels for procedures:

```rust
use cratestack_core::TransactionIsolation;

let isolation = TransactionIsolation::parse("serializable")?;
assert_eq!(isolation.as_sql(), "SERIALIZABLE");
```

## License

MIT