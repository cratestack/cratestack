# embedded-webhook

Single-binary HTTP webhook receiver on top of `include_embedded_schema!` + `cratestack-rusqlite`. The interesting bit is the **inversion**: the only "server with state" example shipped before this was [`server_basic`](../../crates/cratestack/examples/server_basic.rs) — `include_server_schema!` + a Postgres pool. That's the right shape when you have a tier of API servers in front of a managed Postgres. It is overkill when you have one small process at the edge that just needs to remember a few thousand rows on local disk.

This example fills that gap: **axum + tokio on the outside, SQLite + `ModelDelegate` on the inside**, no Postgres anywhere. Same demonstration of the async/sync seam as [`embedded-daemon`](../embedded-daemon), in the server-shaped half of the design space.

## Layout

```
embedded-webhook/
├── Cargo.toml
├── schema.cstack             # WebhookEvent model
├── src/
│   ├── lib.rs                # include_embedded_schema! + axum Router + handlers
│   └── main.rs               # tokio main + bind + graceful shutdown
└── README.md
```

`lib.rs` exposes `build_router(AppState) -> Router` so tests can hit handlers via `tower::ServiceExt::oneshot` without binding a TCP port. `main.rs` is the thinnest possible wrapper that opens the SQLite file, builds the router, and serves it.

## Schema

```cstack
model WebhookEvent {
  id Uuid @id
  source String
  payload String          // JSON-encoded body
  receivedAt DateTime
  status String           // "pending" | "processed"
}
```

`payload` is stored as a JSON string rather than a typed column — webhook payloads are arbitrary and the goal here is to durably capture *what arrived* before the processing pipeline runs.

## Routes

| Method | Path                          | Purpose                                |
|--------|-------------------------------|----------------------------------------|
| POST   | `/webhooks`                   | Accept a webhook (returns 201 + view)  |
| GET    | `/webhooks?status=&limit=`    | List events, newest first              |
| GET    | `/webhooks/{id}`              | Fetch one by id                        |
| POST   | `/webhooks/{id}/processed`    | Mark as processed                      |
| GET    | `/healthz`                    | Liveness check                         |

## Run

```bash
cargo run -p embedded-webhook-example -- \
  --bind 127.0.0.1:8080 \
  --db /tmp/webhooks.db
```

Then in another shell:

```bash
# POST a webhook
curl -s -X POST http://127.0.0.1:8080/webhooks \
  -H 'content-type: application/json' \
  -d '{"source":"github","payload":{"event":"push","ref":"refs/heads/main"}}'

# List
curl -s http://127.0.0.1:8080/webhooks | jq

# Mark processed
ID=$(curl -s http://127.0.0.1:8080/webhooks | jq -r '.[0].id')
curl -s -X POST http://127.0.0.1:8080/webhooks/$ID/processed | jq
```

`Ctrl-C` triggers a graceful shutdown — `axum::serve(...).with_graceful_shutdown(...)` waits for in-flight requests to complete before exiting.

## The async/sync seam

Every handler that touches SQLite goes through `tokio::task::spawn_blocking`:

```rust
async fn create_webhook(
    State(state): State<AppState>,
    Json(input): Json<NewWebhook>,
) -> Result<(StatusCode, Json<WebhookView>), AppError> {
    let runtime = Arc::clone(&state.runtime);
    let payload = serde_json::to_string(&input.payload)?;
    let row = tokio::task::spawn_blocking(move || {
        let events = ModelDelegate::new(&runtime, &cratestack_schema::WEBHOOK_EVENT_MODEL);
        events.create(CreateWebhookEventInput { ... }).run()
    }).await??;
    Ok((StatusCode::CREATED, Json(WebhookView::from_row(row))))
}
```

Why bother? `RusqliteRuntime` holds a `Mutex<Connection>`. If a handler called `events.create(...).run()` directly on the tokio task it'd block the worker for the full duration of the SQL — and `WAL` writes are usually sub-millisecond, but anything that hits an `fsync` or contends with another writer can stretch into the tens of milliseconds. `spawn_blocking` moves the blocking work onto tokio's dedicated blocking pool, leaving the async worker pool free to accept the next connection.

If you only ever run a single-threaded runtime, this matters less; if you're using `#[tokio::main]` (which defaults to multi-threaded), it matters every time.

## Tests

```bash
cargo test -p embedded-webhook-example
```

Four tests, all in `lib.rs` using `tower::ServiceExt::oneshot` against an in-memory SQLite:

- `healthz_returns_ok` — smoke
- `create_then_list_round_trip` — POST followed by GET round-trips the row
- `mark_processed_advances_status` — `?status=pending` filter drops the row after marking
- `missing_webhook_returns_404` — error path

## When to reach for this shape

- Edge / single-tenant deployments where Postgres operational overhead isn't worth it.
- Sidecar processes (sync workers, dead-letter queues, replay buffers) that need durable local state but don't need a shared database.
- CI / test fixtures — spin up the binary against a `tempfile::NamedTempFile` and you have a real HTTP-speaking service with state.

When **not** to reach for it:

- Multiple replicas need to see the same data — that's the Postgres + `include_server_schema!` story.
- Write throughput exceeds what SQLite can handle on a single writer. Roughly: tens of thousands of writes/sec on a good NVMe with WAL, much less if you're behind an `fsync` per request.

## See also

- [`embedded-daemon`](../embedded-daemon) — the other "async-around-sync" example. Same `spawn_blocking` pattern, daemon-shaped instead of server-shaped.
- [`server_basic`](../../crates/cratestack/examples/server_basic.rs) — the Postgres + `include_server_schema!` counterpart.
- [`embedded-cli`](../embedded-cli) — same `include_embedded_schema!` shape with no async at all.
