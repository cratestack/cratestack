# cratestack-auth-conformance

The `AuthProvider` conformance kit (ADR 0021 §6, cratestack#1243). It drives one provider through
REST, RPC unary, RPC batch (each at the root and under a mount prefix) and MCP Streamable HTTP, and
asserts the same verdict, the same refusal rendering, the canonical request and one call per request
everywhere.

Add it as a dev-dependency of the crate that defines your provider:

```toml
[dev-dependencies]
cratestack-auth-conformance = "0.17"
```

```rust
use cratestack_auth_conformance::{Expected, auth_conformance};

#[tokio::test]
async fn my_provider_conforms() {
    auth_conformance(MyProvider::new(keys))
        .accepts("user-42", |canonical| sign(canonical))
        .refuses("expired", |canonical| expired(canonical), Expected::refusal(&refusal))
        .run()
        .await;
}
```

A signer is given the `Canonical` request (method, path without the mount prefix, query, exact
body) before it is sent. The SSE model subscriptions and the sync endpoints, which need a database,
are not driven yet.
