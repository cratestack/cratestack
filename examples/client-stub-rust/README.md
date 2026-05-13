# client-stub-rust

Standalone HTTP-client binary built on [`cratestack-client-rust`](../../crates/cratestack-client-rust/) via `include_client_schema!`. The "backend service that talks to another backend service" shape.

## What it shows

- `include_client_schema!("schema.cstack")` for HTTP-client code-gen
- No `sqlx`, no `axum`, no procedures, no `FromRow` impls — smallest dep surface for a Rust HTTP consumer
- Typed `cratestack_schema::client::Client::new(runtime)` wrapping a `CratestackClient<CborCodec>`
- Typed call: `client.posts().list(&[("limit", "10")], &[]).await?`
- Schema constants (`MODELS`, `TYPES`, `PROCEDURES`) for inspection / metrics

The generated client treats the remote `.cstack` schema as a contract — your service depends only on the schema file, not on the remote service's database or runtime crates.

## Run

```bash
# Point at any CrateStack service exposing the matching schema:
REMOTE_URL=http://localhost:3000 cargo run -p client-stub-rust-example

# Without REMOTE_URL, the example prints the generated typed surface and exits:
cargo run -p client-stub-rust-example
```

## Tests

```bash
cargo test -p client-stub-rust-example
```

The example's `tests/smoke.rs` verifies the generated types compile, expose the expected schema constants, and round-trip through serde. Real HTTP round-trips against an in-process mock server are exercised in [`crates/cratestack/tests/generated_client_rust.rs`](../../crates/cratestack/tests/generated_client_rust.rs).

## See Also

- [`client-multi-service`](../client-multi-service) — same shape but with **two** `include_client_schema!` calls in one binary (BFF / orchestrator)
- [`microservice-pair`](../microservice-pair) — a service that is both a server (`include_server_schema!`) **and** a client of another service (`include_client_schema!`)
