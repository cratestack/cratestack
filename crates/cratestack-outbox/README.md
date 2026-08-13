# cratestack-outbox

The transactional outbox pattern for CrateStack applications:
[`OutboxClient::persist_in_tx`] writes an event inside the caller's own
Postgres transaction so the two commit atomically; a separate snapshotter
drains them in insertion order via [`OutboxClient::drain`], and
[`axum_handler`] exposes that drain (plus a retention sweep) as two HTTP
endpoints.

## Overview

A service that emits domain events alongside a database write faces the
classic dual-write problem — write the row, publish the event, and hope
nothing crashes in between. The outbox pattern closes that gap by writing
the event as an ordinary row in the *same* transaction as the business
write, so the event exists if and only if that transaction committed.

- [`OutboxClient::persist_in_tx`] — insert an event inside the caller's
  transaction. Use this alongside a business write.
- [`OutboxClient::persist`] — insert an event using a pool connection, for
  callers with no surrounding transaction of their own.
- [`OutboxClient::drain`] — page through events in `id` (UUIDv7,
  insertion-order) ascending order, via an opaque cursor.
- [`OutboxClient::gc_older_than`] — sweep events past a retention cutoff.
- [`axum_handler::drain_handler`] / [`axum_handler::gc_handler`] — the
  above two, exposed as axum handlers with JSON/CBOR content negotiation.

## Why no `include_server_schema!`

This crate deliberately does not use cratestack's own schema macro. See the
crate-level doc comment (`src/lib.rs`) for the full argument; in short, the
downstream crate this was absorbed from generated a typed schema purely to
reach `.pool()` on the resulting handle — every actual read/write already
ran raw `sqlx`, and the generated model's typed accessors and `@@allow`
policy checks were never called from anywhere in the crate. Using
`include_server_schema!` at all would also force a dependency on the
`cratestack-pg` L5 facade for a crate whose real logic sits at L2 — a
placement cost paid for nothing. This crate ships a bare DDL constant
instead ([`OUTBOX_EVENTS_DDL`]) — the same posture `cratestack-sqlx`
already takes for its own internal `cratestack_audit`/`cratestack_migrations`
tables.

## Installation

```toml
[dependencies]
cratestack-outbox = "0.7"
```

## Usage

```rust,no_run
use cratestack_outbox::{NewEvent, OutboxClient};
use cratestack_sqlx::sqlx::PgPool;

# async fn example(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
let outbox = OutboxClient::from_pool(pool.clone());

// Alongside a business write, in the same transaction:
let mut tx = pool.begin().await?;
// ... your own business-row insert against `&mut tx` ...
outbox
    .persist_in_tx(
        &mut tx,
        NewEvent::new("review", "rev_123", "review.approved", serde_json::json!({"body": "..."})),
    )
    .await?;
tx.commit().await?;
# Ok(())
# }
```

Provisioning the table — copy [`OUTBOX_EVENTS_DDL`] into your own migration
rather than depending on a shared migrator:

```rust,no_run
# async fn example(pool: cratestack_sqlx::sqlx::PgPool) -> Result<(), Box<dyn std::error::Error>> {
use cratestack_sqlx::sqlx::Executor;

pool.execute(cratestack_outbox::OUTBOX_EVENTS_DDL).await?;
# Ok(())
# }
```

Mounting the drain/GC endpoints:

```rust,no_run
use axum::Router;
use axum::routing::post;
use cratestack_outbox::{OutboxClient, axum_handler};

# fn example(outbox: OutboxClient) -> Router {
Router::new()
    .route("/internal/events/drain", post(axum_handler::drain_handler))
    .route("/internal/events/gc", post(axum_handler::gc_handler))
    .with_state(outbox)
# }
```

## See Also

- `cratestack-sqlx` — `AUDIT_TABLE_DDL`/`MIGRATIONS_TABLE_DDL` are the
  precedent this crate's own `OUTBOX_EVENTS_DDL` copies, and
  `run_in_isolated_tx_with_retries`/`cool_error_from_sqlx` are what this
  crate's writes run through.
- `cratestack-service` — the sibling absorption this one's layer placement
  and `postgres`-dependency reasoning follows.

## License

MIT
