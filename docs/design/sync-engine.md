# Offline sync engine

Status: **prototype**. The prototype is the `cratestack-sync` crate (`publish = false`) and
`examples/sync-notes`. The decisions recorded here are ADR 0020, *Proposed*, in
`cratestack-docs/internals/sync-engine-adr.md`.

## 1. The ask

1. A mutation made on a client reaches the server automatically.
2. A mutation made on the server reaches the clients automatically, whatever code path made it.
3. Clients work offline: reads and writes keep working, and they converge when reconnected.
4. A **TTL**: offline state cannot live forever. That covers stale writes, idle clients, and the
   server-side history kept for them.
5. A **protection mechanism**: the client is untrusted. The embedded role enforces no policy
   (`include_embedded_schema!` ignores `@@allow` by design), so anything a device claims has to be
   re-checked, rate-limited and bounded on the server.

The roadmap has listed "sync engine design" as the blocker for embedded `@@audit`/`@@emit`.
`examples/react-nextjs-daisyui/web/components/useSync.ts` is a hand-rolled version. It uses an
`updatedAt` timestamp cursor with last-write-wins, and it is the baseline this design improves on.

## 2. Non-goals (for now)

- Peer-to-peer or multi-master sync. The server is the only authority.
- Collaborative text or CRDT merge. Fields that need it can adopt a CRDT later, behind the same
  mutation surface.
- Arbitrary partial replication. A client replicates *scopes* (§4.4), not arbitrary queries.
- Non-Rust device runtimes. The client engine is Rust, and reaches Flutter, wasm, Tauri and Expo the
  same way the embedded role already does. A Dart or TypeScript re-implementation is a later
  question (§11).

## 3. The model: server-authoritative mutation replay

Devices send **named mutations** (`note.update {id, title}`), not row snapshots. The server re-runs
each one through a handler written against the generated delegates, inside the push transaction,
under the caller's `CratestackContext`. State only flows back down through the change log.

Alternatives considered:

| Option | Why not |
| --- | --- |
| Row-state sync with last-write-wins (what `useSync.ts` does) | Trusts the client's rows and clock. Bypasses policies and validators. Loses concurrent edits to different fields. Its timestamp cursor also skips rows (§4.2). |
| CRDTs everywhere | Solves merge, not authorization: a CRDT op is still an untrusted write. Most CrateStack data is relational, with invariants a CRDT cannot express. |
| Logical replication / CDC (`wal2json`, Debezium) | Infrastructure the framework cannot assume. Replicates storage rows, not policy-filtered views. Says nothing about the upstream half. |

Replay keeps the server's existing rules the single source of truth. A pushed mutation is no more
powerful than the same call through a generated route.

## 4. Server → device: the change log

### 4.1 Capture by trigger

`CaptureSpec { table, model, pk_column, scope_sql }` installs an `AFTER INSERT OR UPDATE OR DELETE`
trigger. The trigger calls `cratestack_sync_record(scope, model, pk, op)` (`server/ddl.rs`). Because
it is a trigger, writes from sync pushes, generated REST/RPC routes, procedures, migrations and
`psql` are all captured. Ask 2 needs no application code. The example demonstrates it: its server
panel PATCHes through the generated REST route, and the owner's devices update.

The log is **compacted**: one row per `(scope, model, pk)`, and each write moves that row to a new
position. A device that was offline for a month receives each changed row once.

The row's *contents* are not logged. The pull fetches the current row through the application's
`RowFetcher`, which uses the generated delegate with the puller's context, so the **read policy is
applied at pull time**. A row the puller may no longer read arrives as a delete. The log never holds
a value a client is not currently allowed to see.

A row whose scope changes (`UPDATE … SET owner_id`) gets a tombstone in the old scope and an upsert
in the new one, in that order. That is how losing access becomes a delete on the device.

### 4.2 The cursor: why `(txid, seq)` and the xmin gate

A cursor that is a plain sequence or a timestamp skips rows. Transaction A takes position 1,
transaction B takes position 2, and B commits first. A client pulling between the two commits moves
its cursor to 2. When A commits, its row sits *behind* the cursor and is never sent. The outbox's
UUIDv7 drain order has the same property.

The cursor is `(txid, seq)`, where `txid` is the writer's `pg_current_xact_id()` (`xid8`). A pull
only returns positions with `txid < pg_snapshot_xmin(pg_current_snapshot())`, that is, written by
transactions older than every transaction still in flight. Nothing can later commit behind a cursor
handed out under that rule. `tests/server_pg.rs::a_transaction_that_commits_late_is_not_skipped`
holds A open, commits B, and asserts the pull returns *nothing* until A commits, then both, in order.

The cost:

- **Transaction ids and snapshots are cluster-wide.** A long-running transaction in *any* database
  on the same Postgres server delays every pull until it ends. Changes are delayed, never lost. We
  found this the hard way: the late-commit test made unrelated tests in other databases flake until
  the suites were serialized (`tests/support/pg.rs`).
- **Mitigation:** run sync on a cluster where `idle_in_transaction_session_timeout` and
  `statement_timeout` are set. A production deployment should alert on transaction age.
- A page never splits a transaction. It is extended to the end of the last transaction it touches,
  so a device never applies half of one, and a page can exceed `max_pull_changes` by the size of
  that transaction.

### 4.3 Pokes

`cratestack_sync_record` calls `pg_notify('cratestack_sync', scope)`. Postgres delivers it at
commit, never for a rollback, and de-duplicates it within a transaction. `PokeHub` `LISTEN`s and
fans out to `GET /sync/poke` server-sent-event streams, filtered to the caller's scopes. A poke
carries no data: the device pulls, so pulling stays the only path rows take, and missing a poke
costs nothing. After the listener reconnects, it pokes every stream once.

### 4.4 Scopes

`ScopeResolver: Fn(&CratestackContext) -> Option<SyncPrincipal { id, scopes }>`. The server derives
the scopes; a device never names one. The example uses `user:<id>`. A shared notebook would add
`notebook:<id>` scopes from a membership lookup. Scopes decide what a device is *told about*; the
read policy (§4.1) decides what it *receives*. Those are two independent checks.

## 5. Device → server: push

`POST /sync/push {clientId, mutations: [{seq, name, args, createdAt}]}`, in one transaction
(`server/push.rs`):

1. Lock the client row and check it belongs to the caller's principal.
2. Seqs at or below `last_mutation_seq` are retries. They are answered `alreadyApplied` and not
   re-executed. The rest must continue from `last_mutation_seq + 1` with no gap, or the push gets
   `409 sequence_gap` and the server's position.
3. Each mutation runs in a **savepoint**. A rejection (policy, `@version`, validation, unknown name,
   TTL) rolls back that mutation's writes only. It is reported as `rejected {code, message}` and
   still consumes its seq. An infrastructure error aborts the whole push, and the device retries
   the same seqs.
4. `last_mutation_seq` advances, and an **ack** is written to the change log in the device's private
   scope `client:<id>`, in the same transaction as the writes it acknowledges.

Acks travel in the change stream rather than only in the push response. A device must not drop a
pending mutation until it can also see the server state that includes it. Otherwise the row would
briefly revert on screen. The ack and the writes share a transaction, and a page never splits one
(§4.2), so they arrive together by construction.

## 6. Device: the undo log and rebase

The device maintains one invariant:

```text
local tables == server state as of the cursor  +  replay(pending mutations, in seq order)
```

Every pull, rejection or reset **rewinds** to server state, changes it, then **replays** the pending
queue. Rewinding uses an undo log built by SQLite triggers (`client/ddl.rs`). While a local mutator
runs, a `BEFORE UPDATE/DELETE` trigger copies the row into `cratestack_sync_shadow_<table>` (on first
touch only), and every touched key is recorded in `cratestack_sync_dirty`. To rewind: delete every
dirty key, re-insert the shadows. The shadow holds the row *as stored*, so there is no re-encoding
and no storage-class drift.

The naive alternative is to re-apply pending mutations after a pull without rewinding first. That is
only correct for idempotent mutators. A pending `increment` re-applied to state that already
includes it counts twice. `client/tests_rebase.rs` uses a deliberately non-idempotent `inc` mutator
to prove the difference. Rejections need the rewind too: the rejected mutation's optimistic effect
has to disappear without a refetch.

Costs and limits:

- The shadow tables hold one pre-image per dirty row. Storage is proportional to the offline queue,
  not the dataset.
- A mutator must be a pure function of `(db, args)`. The example passes the timestamp *in the args*
  for exactly this reason.
- A primary-key change inside a mutator is recorded under both keys but is otherwise untested.
- A shadow table is rebuilt from the base table's columns only when it is empty, so a schema
  upgrade with a non-empty queue keeps the old shadow columns. Open question (§11).

`unsynced_keys(model)` reads the dirty table, which gives the UI an exact "not synced yet" marker
per row.

## 7. TTLs and protections

All of these are in `SyncPolicy` and checked by `validate()` at build time. Each row names the test
that proves it.

| Concern | Mechanism | Test |
| --- | --- | --- |
| Stale offline intent | `mutation_ttl` (default 7 d): `createdAt` older than this → `rejected: expired` | `ttl_protection_pg::mutation_ttl_rejects_stale_intent_and_post_dated_mutations` |
| Post-dating to dodge the TTL | `createdAt` more than `max_clock_skew` in the future → `rejected: invalid` | same |
| Idle devices | `client_ttl` (default 30 d): GC evicts the registration and its acks; next call → `410 client_expired` | `ttl_protection_pg::client_ttl_evicts_idle_clients_and_their_acks` |
| Double-apply after eviction | Device side: mutations consumed but unacknowledged at eviction are surfaced as rejected and **not** re-sent under the new id; the rest are renumbered from 1. Server side: `client_ttl > mutation_ttl` is enforced, so anything an evicted registration consumed is expired on arrival anyway. | `tests_lifecycle::client_expiry_surfaces_unacknowledged_work_and_renumbers_the_rest`, `policy::rejects_client_ttl_not_exceeding_mutation_ttl` |
| Unbounded history | `tombstone_ttl` (default 30 d): purge deletes, move the horizon; a cursor behind it → `410 cursor_expired` → full resync | `ttl_protection_pg::tombstone_ttl_expires_cursors_that_could_have_missed_a_delete` |
| Unauthenticated callers | Every endpoint runs the app's own `AuthProvider`; no principal → `401` | `ttl_protection_pg::the_http_surface_authenticates_rate_limits_and_maps_status_codes` |
| Hijacking a device's sequence or acks | Client id is server-issued and bound to its principal; any other principal → `403 client_mismatch` (without naming the owner) | `ttl_protection_pg::a_client_id_is_useless_to_another_principal` |
| Reading another scope | Scopes come from the principal; the read policy is re-applied per pulled row | `server_pg::writes_from_any_path_reach_only_their_owners_scope`, `…leaving_a_scope_or_becoming_unreadable_arrives_as_a_delete` |
| Writing past policy | Replay through generated delegates under the caller's context; savepoint per mutation | `server_pg::a_rejected_mutation_rolls_back_only_its_own_writes`, `end_to_end` (Bob renaming Alice's todo) |
| Replay / reordering | Strict contiguous seqs; retries acknowledged, never re-executed | `server_pg::push_applies_retries_are_acknowledged_not_reapplied_and_acks_ride_the_stream`, `…a_gap_in_the_sequence…` |
| Flooding | Body cap before decode; mutations per push; bytes per mutation; pull page cap; registrations per principal (advisory-locked count); poke streams per principal; device queue length | `ttl_protection_pg::registrations_and_pushes_are_bounded`, `poke::tests::stream_slots_are_capped…`, `tests_lifecycle::queue_is_bounded_while_offline` |
| Request rate | Optional per-principal token bucket on every operation through any `RateLimitStore` (in-memory or Redis); fails **closed** if the store errors | `ttl_protection_pg::the_http_surface…` (429 + `Retry-After`) |
| Error detail leakage | Internal errors are logged, and the wire gets `internal error` | (by inspection: `server/mod.rs::internal`) |
| Account switch on a device | `client_mismatch` / `unauthenticated` map to `Recovery::Halt`. The engine never pushes one principal's queue as another's; the host decides (re-login or wipe). | `client/recovery.rs` |

**Not yet covered:** device-key binding (a stolen bearer token plus a stolen client id still works,
which COSE device signatures would close); a server-side cap on change-log size per scope; a
security review.

## 8. Proposed schema surface

The prototype's registries are hand-written: `examples/sync-notes/server/src/sync.rs` on the server,
`client/src/app.rs` on the device. Both are mechanical, and that is the case for generating them:

```text
model Note {
  id      Uuid @id          // sync requires a client-supplied primary key
  ownerId String
  ...
  @@sync(scope: "user:" + ownerId)
}
```

`@@sync` would generate:

- **Server:** the `CaptureSpec`, with `scope_sql` rendered from the expression; a `RowFetcher` that
  is `find_many().where_(pk.in_(..)).run(ctx)`; and default mutators `<model>.create`, `.update` and
  `.delete` over the delegates.
- **Device:** the `DescriptorModel` registration and the matching local mutators.
- Custom mutations become procedures marked `@sync`, replayed through the procedure's existing
  policy.

Parse-time rules: `@@sync` requires a non-generated primary key (an offline create needs its final
id), forbids `@server_only` fields in the scope expression, and is refused in a `db = None` schema.
The four macros stay disjoint. Each emits only its own role's half, and only
`include_server_schema!(db = Postgres)` and `include_embedded_schema!` accept `@@sync`.

**Transport parity.** The sync endpoints are their own protocol, mounted next to either transport,
so a REST schema and an RPC schema get the same sync surface. They are not part of the application
API that the parity rule covers. Whether the endpoints should also be addressable as RPC op ids
(`sync.push`, …) is an open question (§11), not an omission.

## 9. Where it sits

`cratestack-sync` is **L2** (`docs/adr/layers.toml`): `cratestack-core` always, `cratestack-sqlx`
behind `server`, `cratestack-rusqlite` behind `client`. Its axum router uses plain `axum`, as
`cratestack-outbox` does, not `cratestack-axum`. The halves are feature-disjoint, so a device build
never links sqlx and a server build never links rusqlite, consistent with ADR 0013. The client compiles to
`wasm32-unknown-unknown` (`cargo clippy -p cratestack-sync --features client --target
wasm32-unknown-unknown` is clean; the example ships it to the browser).

## 10. What the prototype proved

- **31 tests:** 17 unit tests (protocol, policy, rebase, lifecycle) and 14 against Postgres
  (`server_pg`, `ttl_protection_pg`, and `end_to_end`, which runs two offline-capable devices, a
  third principal and a server-side SQL write until all converge).
- **`examples/sync-notes`**, driven in headless Chromium: three wasm/OPFS devices; offline edits; a
  server-side REST write; reconnect and convergence; a server rejection rolled back locally;
  reload from OPFS.

## 11. Open questions

1. **Codec.** The prototype speaks JSON. The protocol types are plain serde, so CBOR (the framework
   default) is a codec swap.
2. **RPC addressing** of the sync endpoints (§8).
3. **Pokes over HTTP/1.1** cost one connection per device. That is fine for one device per browser,
   and tight for the demo's three panes in one page (example README). Options: HTTP/2, or a shared
   WebSocket per page.
4. **Long transactions** stall pulls cluster-wide (§4.2). Should the server expose the oldest
   in-flight transaction as a metric, or fall back to a timeout-bounded gate?
5. **Schema evolution with a non-empty queue.** Pending mutations written by app version *n* reach a
   server on *n+1*. Mutation names are the compatibility surface, so they need versioning rules.
6. **First sync of a large scope** is a full paged pull. Snapshots or bootstrap files may be needed.
7. **Merge beyond reject.** `@version` gives reject-on-conflict. Field-level merge or a CRDT field
   type would be per-model opt-ins.
8. **Rows in several scopes** (`scope_sql` returns one scope today). An array-valued scope is a
   small change to the trigger and the log key.
9. **Non-Rust device runtimes** (pure Dart or TypeScript), if the Rust engine over FFI or wasm turns
   out to be the wrong fit for some apps.

## 12. Phasing

1. Land the prototype as-is (`publish = false`) and agree on ADR 0020.
2. `@@sync` grammar and codegen for both roles (§8); delete the hand-written registries from the
   example.
3. CBOR, RPC addressing, metrics (transaction-age gate, log size per scope).
4. Publish once a security review has signed off.
