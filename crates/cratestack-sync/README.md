# cratestack-sync

**Prototype. `publish = false`.** It implements
[ADR 0020](https://cratestack.dev/internals/sync-engine-adr) (*Proposed*); the design is in
[`docs/design/sync-engine.md`](../../docs/design/sync-engine.md). The registries are hand-written
until the proposed `@@sync` schema attribute exists. Expect breaking changes.

Offline sync between `include_embedded_schema!` devices and an `include_server_schema!(db = Postgres)`
server:

- **Device → server.** Writes apply locally first and queue as *named mutations*. The server replays
  them through the generated delegates, one savepoint each, so the schema's policies, validators and
  `@version` checks apply. Delivery is exactly-once: a strict per-client sequence, and retries are
  acknowledged without re-running.
- **Server → device.** A Postgres trigger records every write to a synced table, whatever made it:
  sync, a generated REST/RPC route, a procedure or `psql`. Pulls read a compacted change log through a
  commit-ordered `(txid, seq)` cursor. Rows are fetched at pull time through the model's read policy.
  `NOTIFY` pokes connected devices to pull.
- **Offline.** The device keeps an undo log (shadow tables filled by triggers), so a pull or a
  rejection rewinds to server state and replays the pending queue exactly. Non-idempotent mutators
  are safe.

## Features

| Feature | Brings | Use in |
| --- | --- | --- |
| *(none)* | `protocol` (wire types), `SyncPolicy` | anything |
| `server` | `server::{SyncServer, SyncRegistry, sync_router, CaptureSpec, …}`, sqlx + axum | the `cratestack-pg` service |
| `client` | `client::{SyncEngine, DescriptorModel, …}`, rusqlite; no networking, compiles to wasm32 | the `cratestack-sqlite` app |

The two halves are feature-disjoint, so a device build never links sqlx and a server build never
links rusqlite.

## TTLs and protections

TTLs, all in `SyncPolicy` and enforced by `SyncServer::gc` plus the push path:

| TTL | Effect |
| --- | --- |
| `mutation_ttl` | A mutation older than this is rejected as `expired`; stale offline intent does not land. |
| `client_ttl` | An idle client is evicted. It gets `client_expired`, re-registers, and its unacknowledged mutations come back as rejected (never re-sent under a new id). Must exceed `mutation_ttl`; `validate()` enforces it. |
| `tombstone_ttl` | Deletes age out of the log. A cursor older than the purge horizon gets `cursor_expired` and a full resync. |

Protections:

- Every endpoint requires the app's own `AuthProvider`.
- A client id is bound to the principal that registered it.
- Scopes are derived server-side from the principal.
- Unknown mutations are rejected.
- Requests are limited by body size, mutations per push, args size, pull page size, clients per
  principal, poke streams per principal and the device's offline queue length.
- Optional per-principal rate limiting through any `RateLimitStore`. It fails closed when the store
  is unreachable.

The full list, each with the test that proves it, is in the design doc, §7.

## Tests

```bash
cargo test -p cratestack-sync --features client,server                # unit tests; PG suites skip
CRATESTACK_TEST_DATABASE_URL=postgres://… CRATESTACK_REQUIRE_DB=1 \
  cargo test -p cratestack-sync --features client,server              # + server_pg, ttl_protection_pg, end_to_end
```

The runnable demo is [`examples/sync-notes`](../../examples/sync-notes).
