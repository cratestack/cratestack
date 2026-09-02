# rpc-client

Pure RPC HTTP client consuming a [`transport rpc`](../rpc-procedures/README.md) server. Owns no
database and emits no router — it only speaks `POST /rpc/{op_id}` and `POST /rpc/batch` over
HTTP/CBOR to a remote `rpc-procedures-example` service.

## What it shows

- `include_client_schema!("schema.cstack")` treated as a **contract** against a shared
  `transport rpc` schema — the generated `cratestack_schema::client::Client` types line up 1:1
  with the op-ids the server dispatches (`procedure.greet`, `procedure.increment`).
- **Unary call** — `client.procedures().greet(&args)` returns a
  `BatchableCall`; `.await` fires one `POST /rpc/procedure.greet` immediately.
- **Batch call** — `client.batch()` returns a `BatchBuilder`; prep a few `BatchableCall`s,
  `.queue()` each, then one `POST /rpc/batch` round-trip and collect each result by its
  `BatchHandle`.
- A `RequestAuthorizer` that injects `x-auth-id` on every request so the server's
  `@allow(auth() != null)` gate passes.
- The pure **`cratestack-client` facade** — the first committed client example to use it (see
  the `Cargo.toml` note); `axum`/`tower`/`hyper` are structurally absent from the shipped
  dependency graph.

This example is the client bookend to the `rpc-procedures` server example (unary + batch over
the *unary* surface). For streaming (`procedure ...: T[]` → `RpcStream`), see
[`rpc-streaming-client-rust`](../rpc-streaming-client-rust/).

## Run

```bash
# In one terminal:
cargo run -p rpc-procedures-example

# In another:
REMOTE_URL=http://localhost:3000 cargo run -p rpc-client-example
```

Without `REMOTE_URL` the binary prints what it would do and exits.

## Tests

The crate ships a `tests/smoke.rs` that spawns the **real** `rpc-procedures-example` router
in-process (as a dev-dependency — `axum` appears only under `[dev-dependencies]`) and drives it
through the typed client:

```bash
cargo test -p rpc-client-example
```

Runs in CI via the standard `cargo test --workspace`.
