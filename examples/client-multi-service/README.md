# client-multi-service

BFF / orchestrator: one binary that fans out to **two** upstream CrateStack services. Each upstream contributes its own typed client surface generated from its own `.cstack` schema.

## What it shows

- **Two `include_client_schema!` calls in the same binary**, isolated in separate modules (`billing::cratestack_schema` and `inventory::cratestack_schema`) so the emitted modules don't collide
- Each module produces an independent generated `Client`, fully typed against its remote service
- Concurrent fan-out with `tokio::try_join!` — the canonical BFF shape
- Schema files (`schemas/billing.cstack`, `schemas/inventory.cstack`) treated as contracts; this binary owns neither database

## Run

```bash
BILLING_URL=http://billing.internal:3000 \
INVENTORY_URL=http://inventory.internal:3000 \
cargo run -p client-multi-service-example

# Without env vars, prints the typed surfaces for both services and exits:
cargo run -p client-multi-service-example
```

## Tests

```bash
cargo test -p client-multi-service-example
```

The smoke test asserts that each module's `cratestack_schema::MODELS` is disjoint and that the generated types round-trip through serde independently — guarding against the failure mode where one schema's types accidentally leak into the other module.

## When to use this pattern

- A web BFF that aggregates calls to multiple internal CrateStack services
- An admin tool that needs to interact with several services through their typed contracts
- A scheduled job that pulls data from one service and pushes to another

## See Also

- [`client-stub-rust`](../client-stub-rust) — single-upstream variant
- [`microservice-pair`](../microservice-pair) — a service that is **both** a server and a client of another
