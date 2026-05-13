# microservice-pair

A service that is **both** a server and a typed HTTP client of another service. Demonstrates the "orders service that calls the catalog service" shape that nearly every realistic CrateStack microservice deployment ends up needing.

## What it shows

- **`include_server_schema!("schemas/orders.cstack", db = Postgres)`** at the crate root — this service is the system of record for `Order`s, owns its own Postgres database, and serves the generated axum router.
- **`include_client_schema!("schemas/catalog.cstack")`** inside a `catalog_client` module — this service consumes the `Catalog` service's typed contract to validate product references before persisting orders.
- **Strict separation**: server-owned models and upstream contracts live in different modules (`cratestack_schema` vs `catalog_client::cratestack_schema`), so the "owned vs upstream" boundary is visible at every call site.
- A small `CatalogClient` wrapper showing where you'd add retries / circuit breaking / metrics around generated client calls in production.

This is the canonical pattern for one service to depend on another in a CrateStack mesh. The dependency is on the upstream's `.cstack` file as a *contract* — not on its database, not on its runtime crates.

## Run

```bash
export DATABASE_URL=postgres://cratestack:cratestack@localhost/orders
export CATALOG_URL=http://catalog.internal:3000
cargo run -p microservice-pair-example
# orders-service listening on http://127.0.0.1:3001

# Without env vars, prints both surfaces (server + client) and exits:
cargo run -p microservice-pair-example
```

## Tests

```bash
cargo test -p microservice-pair-example
```

The smoke tests assert:

1. The owned-server and upstream-client modules expose disjoint `MODELS` (no leakage between `Order` and `Product`).
2. The router builds offline against a lazy `PgPool` — proves the macro-generated wiring is sound before you start hunting a database.

## See Also

- [`server_basic`](../../crates/cratestack/examples/server_basic.rs) — pure server, no upstream
- [`client-stub-rust`](../client-stub-rust) — pure client, no owned database
- [`client-multi-service`](../client-multi-service) — pure client, fans out to **two** upstreams
