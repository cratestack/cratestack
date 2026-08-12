# cratestack-core

Core types, traits, and error handling shared across the CrateStack workspace.

## Overview

`cratestack-core` provides the foundational types that the rest of the workspace depends on:

- **Error handling**: `CoolError` with HTTP status mapping and operator vs. public message split
- **Auth context**: `CoolContext`, `PrincipalContext`, `CoolAuthIdentity`, `AuthProvider`
- **Schema AST**: `Schema`, `Model`, `Field`, `Procedure`, `MixinDecl`, `TypeDecl`, `EnumDecl`
- **Audit**: `AuditEvent`, `AuditOperation`, `AuditActor`, `AuditSink`, `NoopAuditSink`, `MulticastAuditSink`
- **Signed envelope**: `HmacEnvelope` (HS256), `KeyProvider`, `StaticKeyProvider`, `NonceStore`, `InMemoryNonceStore`
- **Codec/envelope traits**: `CoolCodec`, `CoolEnvelope`, `NoEnvelope`
- **Event bus**: `CoolEventBus`, `ModelEvent<T>`, `ModelEventKind`, `CoolEventEnvelope`
- **Transaction isolation**: `TransactionIsolation`
- **Decimal scalar**: `Decimal` (compile-time backend)
- **Validators**: `validate_length`, `validate_range_i64`, `validate_range_decimal`, `validate_email`, `validate_uri`, `validate_iso4217`

## Installation

```toml
[dependencies]
cratestack-core = "0.6.7"
```

At most one `Decimal` backend feature may be selected — `decimal-rust-decimal` (the default, `Copy`, fixed 96-bit precision) or `decimal-bigdecimal` (arbitrary precision, heap-allocated, not `Copy`). Selecting both is a compile error. Selecting neither is allowed as of cratestack#521: `Decimal` is simply not exported, which lets a consumer narrow its dependency graph with `default-features = false` without naming a backend it never uses. A schema that does use a `Decimal` field then fails with rustc's own "cannot find type `Decimal`" rather than a clearer message from this crate.

## Error Handling

`CoolError` returns a safe public message to clients while keeping operator-only detail for tracing.

```rust
use cratestack_core::CoolError;
use http::StatusCode;

let err = CoolError::BadRequest("missing query parameter".to_owned());
assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
assert_eq!(err.public_message(), "missing query parameter");

// 5xx variants return a fixed canned public message; the inner string flows to `detail` only.
let err = CoolError::Database("connection refused".to_owned());
assert_eq!(err.public_message(), "internal error");
assert_eq!(err.detail(), Some("connection refused"));
```

Variants: `BadRequest`, `NotAcceptable`, `Unauthorized`, `UnsupportedMediaType`, `Forbidden`, `NotFound`, `Conflict`, `Validation`, `PreconditionFailed`, `Codec`, `Database`, `DatabaseTyped`, `Internal`. The codec/database/internal variants are 5xx-mapped. `CoolError` is `#[non_exhaustive]`, so downstream matches must include a wildcard arm.

`DatabaseTyped` carries a `DbErrorInfo { detail, sqlstate, constraint }` and is produced by `cratestack_sqlx::cool_error_from_sqlx` at sqlx call sites. Use `err.db_sqlstate()` and `err.db_constraint()` to inspect the typed fields instead of substring-matching the stringified detail.

## Auth Context

`CoolContext` carries the authenticated principal and arbitrary host-provided extensions.

```rust
use cratestack_core::{CoolContext, Value};

let ctx = CoolContext::anonymous();

let ctx = CoolContext::from_principal(Some(serde_json::json!({
    "id": "usr_123",
    "role": "admin",
    "tenant": { "id": "org_456" }
})))?;

assert_eq!(ctx.auth_field("id"), Some(&Value::String("usr_123".to_owned())));
assert_eq!(ctx.tenant_id(), Some("org_456"));
```

### AuthProvider

Host applications implement `AuthProvider` to resolve auth from HTTP requests:

```rust
use cratestack_core::{AuthProvider, CoolContext, CoolError, RequestContext};

#[derive(Clone)]
struct MyAuthProvider;

impl AuthProvider for MyAuthProvider {
    type Error = CoolError;

    async fn authenticate(&self, request: &RequestContext<'_>) -> Result<CoolContext, Self::Error> {
        let token = request.headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| CoolError::Unauthorized("missing token".to_owned()))?;
        // Validate and project into CoolContext...
        Ok(CoolContext::anonymous())
    }
}
```

`AuthProvider` is also implemented blanket for any `Fn(&HeaderMap) -> Result<CoolContext, E>` closure.

## Audit Events

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

`AuditSink` is an async trait. The bundled implementations are `NoopAuditSink` and `MulticastAuditSink`. The in-database table written by `cratestack-sqlx` is treated as the canonical record; sinks are best-effort projections.

## Decimal Backend

```toml
[dependencies]
cratestack-core = { version = "0.6.7", default-features = false, features = ["decimal-rust-decimal"] }
# or: features = ["decimal-bigdecimal"]
```

```rust
use cratestack_core::Decimal;

let amount: Decimal = "123.45".parse()?;
```

Both backends implement `Clone`, `Debug`, `Display`, `FromStr`, `PartialEq`, `PartialOrd`, `Ord`, `Eq`, `Hash`, and `Default`, so generated code and downstream crates never branch on which one is active. The one difference that *does* leak through: `rust_decimal::Decimal` is `Copy`, `bigdecimal::BigDecimal` is not (it heap-allocates) — code holding a `Decimal` by value needs `.clone()` where it used to rely on an implicit copy.

### ⚠️ Cross-backend wire compatibility — a real deployment constraint, not a footnote

For **ordinary** values (anything within `rust_decimal`'s ~28-29 significant-digit capacity), the CBOR and JSON wire bytes produced by the two backends are **byte-identical** — a `decimal-bigdecimal` server and a `decimal-rust-decimal` peer interoperate transparently as long as every value stays in range. Verified: `serde_json::to_string` of `BigDecimal::from_str("123.45")` and `rust_decimal::Decimal::from_str("123.45")` both produce `"123.45"`.

**Past that range, they do not.** `bigdecimal::BigDecimal`'s `Display`/serde output switches to scientific notation once the value's scale exceeds a threshold — e.g. `BigDecimal::from_str("0.00000000000000000000000000001")` (1e-29, one order of magnitude past what `rust_decimal` can represent) serializes as the string `"1E-29"`. `rust_decimal::Decimal::from_str("1E-29")` on the receiving end returns `Err(ScaleExceedsMaximumPrecision(29))` — it does not silently round or truncate, it hard-fails to decode.

**Still a hard failure between two `decimal-rust-decimal` peers** — that backend's own `Decimal::from_str` never accepts scientific notation, so a `decimal-rust-decimal` server that (mistakenly, or via a hand-rolled client) receives `"1E-29"` fails to decode it, and a `decimal-rust-decimal` *client* talking to a `decimal-bigdecimal` server hits the same wall on the way in. Nothing in this crate changes that; it is a real constraint on the Rust-to-Rust wire format, not a client-generation gap.

**cratestack#498/#499 (generated clients):** the shipped Dart and TypeScript client SDKs (`cratestack-client-dart`, `cratestack-client-typescript`) used to only ever treat `Decimal` as an opaque wire-format string, which made the scientific-vs-positional-notation split above a real, silent correctness hazard for any non-Rust client — the string form of the *same value* changed depending on which backend built the server, and neither SDK could parse, compare, or do arithmetic on it. As of the `Decimal`-type generator work (cratestack#498/#499, **breaking** — see each package's own migration note), both SDKs generate a real arbitrary-precision decimal type (`package:decimal` for Dart, `decimal.js` for TypeScript) that parses both notations into the identical value and always *re*-encodes in plain positional notation, so a `decimal-bigdecimal` server is now safe to pair with either generated client regardless of a value's magnitude — including a relation-embedded `Decimal` field, a procedure's own `Decimal`-bearing return type, the TypeScript `swr` preset's decode path, and the generated gRPC-preset clients (Dart via the shared `fromWire`/`toWire` chokepoint every transport already used; TypeScript via a dedicated gRPC-Web `"decimal"` wire kind), all of which are real, executed round-trip test coverage now rather than declared-but-unrevived types.

## Transaction Isolation

```rust
use cratestack_core::TransactionIsolation;

let isolation = TransactionIsolation::parse("serializable")?;
assert_eq!(isolation.as_sql(), "SERIALIZABLE");
```

Accepts `read_committed` / `read committed`, `repeatable_read` / `repeatable read`, and `serializable`.

## Signed Envelope (HMAC-SHA-256)

`HmacEnvelope<K: KeyProvider>` implements `CoolEnvelope` for HS256-signed messages. Production multi-replica deployments back the `NonceStore` with Redis so replay rejection holds cluster-wide.

## See Also

- [Auth Provider guide](https://cratestack.dev/guides/auth-provider)
- [Audit Log guide](https://cratestack.dev/guides/audit-log)
- [Transaction Isolation guide](https://cratestack.dev/guides/transaction-isolation)
- [Validators guide](https://cratestack.dev/guides/validators)

## License

MIT
