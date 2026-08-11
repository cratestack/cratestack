# Changelog

## 0.7.11 (2026-08-11)

### `cratestack-core`: selecting no decimal backend is no longer a hard compile error (#505, #521)

`cratestack-core` used to hard-fail with `compile_error!("enable exactly one decimal backend
feature — decimal-rust-decimal or decimal-bigdecimal")` whenever a consumer built it (directly
or transitively) with `default-features = false` and neither `decimal-rust-decimal` nor
`decimal-bigdecimal` selected. That bit a consumer that legitimately narrows its dependency graph
this way and never uses a `Decimal`-typed field at all — e.g. `cratestack-api` (`provider =
"none"`, no `model` blocks) — forcing it to name a decimal backend it never touches. The break
was invisible in a `cargo check --workspace` run (feature unification from other workspace
members hid it) until the affected crate was built alone — exactly what happened in the field
(ADORSYS-GIS/webank-services#279).

Selecting *both* backends at once is still a hard `compile_error!` — that half of the invariant
is unchanged and stays a graph-wide constraint (documented in `cratestack-core`'s crate-level
rustdoc and in `CLAUDE.md`): two independent dependents in the same build, each individually
well-formed and each deliberately choosing a different backend, can still force an unbuildable
combined graph. Making the two backends genuinely additive (or moving the choice off Cargo
features entirely) remains open, unaddressed by this change, and reserved for a future,
maintainer-scoped design decision.

**What actually changed:** `cratestack-core::Decimal` (and everything in this crate and its
downstream SQL layer that references it unconditionally — `cratestack-core::validate_range_decimal`,
`cratestack-sql::SqlValue::Decimal`, `cratestack-sql`'s `IntoSqlValue for Decimal`, and the
matching bind/decode arms in `cratestack-rusqlite` and `cratestack-sqlx`) is now `#[cfg]`-gated on
"a decimal backend is selected", the same pattern throughout. With neither backend selected,
these symbols simply don't exist on the public surface instead of hard-erroring the whole build —
a consumer that never references `Decimal`, directly or via a schema with no `Decimal`-typed
field, now builds cleanly across every facade (`cratestack-pg`, `cratestack-api`,
`cratestack-sqlite`, `cratestack-client`, plus `cratestack-axum`/`cratestack-studio`), even under
`--no-default-features`. A consumer that *does* try to use `Decimal` without picking a backend
now gets a plain rustc "cannot find type/variant `Decimal`" from wherever the reference lives,
instead of the old, single, clearer `compile_error!` naming the missing choice — a diagnostic
regression accepted in exchange for not hard-failing every backend-agnostic consumer.

No consumer-visible signature or behavior change for anyone who already selects a decimal backend
(the default, `decimal-rust-decimal`, or the opt-in `decimal-bigdecimal`) — this only affects
builds that previously hit the removed `compile_error!`.

### `AuditSink` gets a real installation path (#473, #517)

`cratestack_core::AuditSink` (plus `NoopAuditSink`/`MulticastAuditSink`) has existed since
before this release, but had nowhere to be installed: a consumer could construct a sink and had
no way to hand it to the runtime, and `AuditSink::record` was never invoked anywhere in the
workspace — `cratestack-sqlx/src/audit.rs`'s own module doc claimed fan-out "goes through
`AuditSink`" while that was, in fact, dead code.

`SqlxRuntime` now carries an installable `Arc<dyn AuditSink>` (default `NoopAuditSink`, so
existing `SqlxRuntime::new(pool)` callers see no behavior change), installed via
`SqlxRuntime::with_audit_sink` or, for schema consumers, the macro-generated
`CratestackBuilder::with_audit_sink` — the same shape `IdempotencyStore`/`RateLimitStore` use.
Every `@@audit` write path (`create`/`update`/`delete`/`upsert`, their `_many` and batch
variants) now fans the event out to the installed sink *after* its owning transaction commits,
never from inside it: the in-database `cratestack_audit` row remains the sole in-transaction
write and source of truth (unchanged, no double-write), and a sink is never invoked for a
mutation that ultimately rolled back. Sink errors are logged (`tracing::warn!`), not propagated
— by the time the sink runs, the mutation already committed, so failing the caller's request
over a downstream projection hiccup would be strictly worse than a best-effort delivery.
`run_in_tx` variants (caller-managed transaction) do not fan out, mirroring the existing event
outbox, which has never drained from `run_in_tx` either. **This is a real gap, not just a
deferral**: there is currently no way for a `run_in_tx` caller to opt into sink fan-out
themselves — the dispatch helper is crate-private and no `run_in_tx` variant returns the
`AuditEvent` it would need — so a caller chaining `run_in_tx` calls across a caller-managed
transaction (see `crates/cratestack-pg/tests/banking_chained_audit_tx.rs`) gets the
in-transaction `cratestack_audit` row on commit but a real installed `AuditSink` observes
nothing for that transaction, silently. Worth its own follow-up issue; see
`crates/cratestack-sqlx/src/audit/sink.rs`'s doc comment for the full reasoning. Dispatch is
also sequential, not concurrent, so the added latency of a slow sink is per-row on
`update_many`/`delete_many`/batch paths, not per-request.

### `cratestack-studio`: refuse silent `@version`/`@@emit` bypass on `[target.db]` writes — breaking (#516, cratestack#507)

A write through `cratestack studio` against a `[target.db]` target went straight to SQL: it never
bumped a model's `@version` column and never wrote a `cratestack_event_outbox` row for an
`@@emit`-annotated model, and neither omission was reported anywhere — the request returned `200`
with the updated row. Both consequences are silent and outlive the request: a stale `@version`
still satisfies a later `if_match`, so optimistic concurrency does not fail-safe, it silently does
not apply; and `@@emit` side effects (for example customer-facing delivery webhooks) never fire,
with no trace that one was skipped.

Studio now refuses `POST`/`PATCH`/`DELETE` on a `rw` `[target.db]` target against any model that
declares `@version` or `@@emit(...)`, returning `403 UNSAFE_DB_WRITE` and naming the specific
attribute(s) that triggered the refusal, unless the target sets `allow_unsafe_writes = true` in
`studio.toml`. The refusal runs in the HTTP handler (`require_safe_write`,
`crates/cratestack-studio/src/api/records/guards.rs`) before any `DataSource` call, so it applies
identically to Postgres- and SQLite-backed targets, and models with neither annotation are
unaffected either way. A write allowed only because a target opted in is also loud after the fact,
not just at the moment the config flag is set: it logs a `tracing::warn!` naming the target, model,
and skipped annotation(s), and `AuditEntry` gains an `unsafe_write: bool` field (default `false`,
`#[serde(default)]` so a pre-upgrade JSONL audit sidecar still replays cleanly) so `GET /api/audit`
and the sidecar can distinguish a bypass write from an ordinary one.

The `@@allow` half of the original report — an unauthenticated `[target.db]` read returning a
`@sensitive` field in cleartext — is deliberately left alone here; it is arguably intended for a
direct-DB admin tool and is tracked separately for a maintainer decision, not fixed unilaterally in
this change. Likewise, routing `[target.db]` writes through the same descriptor path the generated
server uses (so `@version`/`@@emit` would actually apply, rather than being refused) remains
unimplemented and is left for a future, larger change.

**Migration.** Any existing `rw` `[target.db]` deployment whose schema declares a model with
`@version` or `@@emit(...)` will start getting `403 UNSAFE_DB_WRITE` on `POST`/`PATCH`/`DELETE`
against that model through Studio. Add `allow_unsafe_writes = true` under that target's
`[target.db]` block in `studio.toml` to keep the previous (silent-bypass) behavior, or leave it
unset and route those writes through `[target.api]` instead, where `@version`, `@@emit`, and
`@@allow` all apply exactly as declared in the schema. Reads and `[target.api]`-only targets are
unaffected either way.

### Per-procedure `@status(<code>)` for REST success responses (#407, #511)

`generate_procedure_axum_handler` hardcoded `axum::http::StatusCode::OK` for every procedure's
`Ok(...)` response, with no schema-level way to declare a different 2xx status (e.g. `202
Accepted` for a submit-and-acknowledge procedure whose real verdict arrives later via webhook).
A schema author can now write:

```
procedure submitKycDocument(args: SubmitKycDocumentInput): KycPresignReply
  @status(202)
```

`cratestack-parser` validates the argument is a real `200..=299` status at schema-compile time
(anything outside that range, or on a `transport rpc` schema — see below — is rejected with a
clear diagnostic, not a runtime surprise); `cratestack-macros` threads the declared status into
`result_encoder` for the unary, `TypeArity::List`, and `@stream` branches alike, replacing the
previously-hardcoded literal in all three. Absent the attribute, codegen is byte-identical to
before (the pre-existing cratestack#283 pinned-token regression test is unchanged). Error
responses are untouched either way — `CoolError`'s own status mapping governs `Err(...)`
unconditionally, independent of `@status`.

`@status` is REST-only and is rejected at schema-compile time on `transport rpc` schemas: RPC
unary dispatch shares the exact same generated handler REST uses, so an unrejected `@status`
there would silently become wire-visible on the RPC response too. `transport grpc` is
unaffected either way — tonic's gRPC status model never reads the inner HTTP status this
attribute controls, so the combination is inert, not wrong, and stays allowed.

Known limitation, left for a follow-up rather than silently narrowed here: `@status(204)` is
accepted by the `200..=299` range check, but the REST encoder always serializes and attaches a
response body regardless of declared status, so a declared `204` currently produces a
`204 No Content` response that carries a body — a protocol violation per RFC 9110 §15.3.5.

### Typed Rust client can read response headers — `*_with_response` methods (#493, #510)

`decode_typed_response` (`cratestack-client-rust/src/client/decode.rs`) read `response.headers`
only to find `Content-Type`, then returned the decoded body alone — every typed call built on it
(`get`/`post`/`patch`/`delete`, and the generated `<Model>Client`'s `list`/`get`/`create`/`update`/
`delete`) discarded every response header. For any `@version` model, that made the typed client
structurally unable to do a concurrency-safe `PATCH`: CrateStack's optimistic-locking contract
requires `If-Match` on that verb, with the current version handed back as `ETag` on `GET` — so
the required round trip, `GET` → read `ETag` → `PATCH` with `If-Match`, had no typed path through
its middle step. The same gap hid `Idempotency-Replayed` (on a replayed create) and `Retry-After`
(on a `429`) from a typed caller. **Note:** `DELETE` is not part of that contract — the server
does not currently enforce `If-Match` on `DELETE` for any model, versioned or not (see below).

Added a `TypedResponse<Output> { value, status, headers }` (with a case-insensitive
`.header(name)` accessor, plus `.header_values(name)` for the rare header that legitimately
repeats, e.g. `Set-Cookie`) and a parallel `*_with_response` method next to every existing typed
method: `CratestackClient::{get,post,patch,delete}_with_response`, and on the generated REST
`<Model>Client`, `get_with_response`/`update_with_response`/`delete_with_response`. Purely
additive — `decode_typed_response` is now implemented in terms of a new
`decode_typed_response_with_metadata`, but keeps its exact original signature and behavior, so
every existing call site (including every already-generated client) keeps compiling and behaving
identically with no changes required.

`delete_with_response` ships alongside `get_with_response`/`patch_with_response` for surface
symmetry (status and headers on every write, not just versioned ones — useful for e.g. reading a
`Retry-After` on a `429`), but unlike `patch_with_response`, sending `If-Match` on a `DELETE` has
**no concurrency-safety effect today**: the server accepts and ignores it. Server-side `If-Match`
enforcement on `DELETE` is a real gap in CrateStack's optimistic-locking story — deliberately
*not* implemented here, since it is a separate feature decision outside this issue's scope, and
reported for its own follow-up issue instead.

Scoped to REST transport. RPC transport (`transport rpc`) has no `ETag`/`If-Match` handling
anywhere server-side — a schema-versioned model's concurrency control there, if any, would need
to travel through the request/response body, not an HTTP header — so there is nothing to wire on
the RPC client's `BatchableCall` surface for this issue. Projection reads (`get_view`/`list_view`/
`list_view_paged`) and `create_with_response` on the generated model client are also left
out-of-scope: the acceptance-driving round trip is `GET` → `ETag` → `PATCH` with `If-Match`, which
`get_with_response`/`update_with_response` cover in full; a create-side `Idempotency-Replayed`
reader is still reachable today via the (now also additive) `CratestackClient::post_with_response`
directly, just not yet wrapped by the generated `<Model>Client::create_with_response`.

Verified: `cargo test -p cratestack-client-rust` (unit coverage of
`decode_typed_response_with_metadata` against hand-built responses, including an `ETag`-shaped
header and a case-insensitive lookup; a real-HTTP-server integration test in
`tests/typed_response.rs` proving `get_with_response` → `ETag` → `patch_with_response` with
`If-Match` round-trips end-to-end, a 412-on-stale-`If-Match` case, and that the plain
`get`/`patch` methods are unchanged) and `cargo test -p cratestack-client` (a new
`tests/generated_client_versioning.rs`, using a schema borrowed verbatim from
`cratestack-pg/tests/fixtures/banking_versioning.cstack`, proving the *generated* `<Model>Client`
reaches the same round trip, not just the underlying runtime).

### `Value` serializes untagged on the wire, matching what it already persists — breaking (#506)

`cratestack_core::Value` derived `Serialize`/`Deserialize`, which emits serde's
externally-tagged enum representation. `Value::String("foo")` went on the wire as
`{"String":"foo"}` rather than `"foo"`, and an empty map as `{"Map":{}}` rather than `{}`.
cratestack#162 / #395 fixed that for a schema `Json` **column** by routing persistence through
`Value::to_plain_json`, but only for the column — every other path still carried the tag:
procedure arguments and results typed `Json`, auth claims, audit payloads, RPC error details.

The practical cost landed on consumers. A `Json?` procedure argument rejected `"foo"` and
required `{"String":"foo"}`, so every caller hand-wrote the tag at every call site, and every
generated Dart and TypeScript client inherited a shape no other JSON or CBOR producer emits.
The persisted shape and the wire shape disagreed for the same value.

`Serialize`/`Deserialize` are now hand-written and untagged (`cratestack-core/src/value/codec.rs`).
`serde_json::to_value(&value)` now produces exactly `value.to_plain_json()`, and
`deserialize_any` accepts whatever a self-describing format hands over. `to_plain_json` /
`from_plain_json` are kept: they are infallible and total (substituting `null` for a NaN float,
which the persistence layer relies on) and they make the on-disk contract explicit at the call
site rather than implicit in a serde impl.

Two format-specific details, both measured against the first-party backends rather than assumed:

- **`Null` serializes via `serialize_none`, never `serialize_unit`.** `minicbor-serde` encodes
  `()` as `0x80` — an empty *array*, not RFC 8949 null — while `None` correctly encodes as
  `0xf6`. `serialize_unit` would have put that non-conformant shape on the wire for any
  `Value::Null` nested in a list or sent as a bare argument. This matches the choice
  `ProjectedValue::Null` already makes for the same reason (#430).
- **`Bytes` branches on `is_human_readable()`.** Binary formats get a native byte string
  (CBOR `0x44 de ad be ef`) and round-trip losslessly. Human-readable formats get the same
  base64 string `to_plain_json` already writes, and inherit the same documented asymmetry —
  a JSON string always decodes back as `Value::String`, because nothing distinguishes base64
  from ordinary text.

**Migration.** Anything that persisted a `Value` through its serde impl rather than through
`to_plain_json` — a custom `AuditSink`, a Redis-backed store — will read old tagged rows as
`Value::Map` with a single variant-named key. Redis-backed state self-heals on TTL expiry.
Callers that hand-wrote the tag to satisfy the old wire format must stop: send `"foo"`, not
`{"String":"foo"}`. Regenerate Dart/TypeScript clients.

### `cratestack-codec-cbor`: corrected a false claim in the encoder comment

The comment asserted that `minicbor-serde` reports `is_human_readable() == true`. It reports
**false** — verified by encoding a probe type whose `Serialize` echoes the hint, which emits
`0xf4`. `cratestack-axum`'s `projection.rs` (#430) already documented the correct behavior, so
the two disagreed. The comment also still described the `Value::Null`-stripping workaround that
#430 removed. No behavior change; the code was right and the comment was wrong.

### Pluralizer gains the standard English `y -> ies` rule — breaking (#504, #509)

`cratestack_core::route_naming::pluralize` (and, via it, `cratestack-migrate::naming::table_name`)
had no `y -> ies` case: any model name ending in a consonant + `y` derived the wrong plural —
`category` -> `categorys`, `webhook_delivery` -> `webhook_deliverys` — instead of the
grammatically correct `categories` / `webhook_deliveries`. This wasn't just cosmetic: the derived
name is the actual SQL table the generated model client queries, so a consumer who hand-wrote a
migration using the correct English plural got `relation "webhook_deliverys" does not exist` the
moment the generated client touched that model — a real production defect downstream
(webank-services' `adminGetWebhooks`; see cratestack#504's linked ADR).

`pluralize` now applies the standard rule: consonant + `y` -> `ies` (`category` -> `categories`);
vowel + `y` (`day`) or anything else -> plain `+s`. `cratestack-migrate::naming::pluralize`, a
second hand-synced copy of the same function that had already drifted apart from this one, is
deleted; `cratestack-migrate::naming::table_name` now calls `cratestack_core::route_naming::pluralize`
directly, so there is exactly one implementation to keep correct going forward.

This changes both generated REST route segments and generated table names for **every**
model/view whose name ends in a consonant + `y`. It does *not* touch
`cratestack-client-typescript::naming` or `cratestack-client-dart::idents`, the two SDK
accessor/method-name generators (`db.categories()`, `useCategories()`) — they already implement
the correct consonant/vowel rule and were deliberately out of scope here.

**Migration.** This is a breaking change to generated table names and REST routes for any schema
with a model/view name ending in a consonant + `y` (`Category`, `Delivery`, `Entry`, `Query`,
...). `cratestack-migrate`'s diff engine matches tables **by name only** and never infers a
rename (`crates/cratestack-migrate/src/diff.rs`) — running `cratestack migrate diff` against a
schema with a deployed `categorys` table, without further action, emits `DropTable(categorys)` +
`CreateTable(categories)`, and applying that migration **destroys the table's data**. Before
running `migrate diff` after upgrading past this change, declare
`@@rename(from = "<old_table_name>")` on every affected model (e.g.
`@@rename(from = "categorys")` on `model Category`) so the diff engine emits
`ALTER TABLE ... RENAME TO ...` instead — verified end-to-end by
`crates/cratestack-migrate/src/emit/postgres/tests/renames.rs`'s
`pluralization_change_with_rename_marker_is_a_rename_not_drop_and_create` test (and its sibling
`..._without_rename_marker_drops_and_recreates`, which pins down the destructive default if this
step is skipped). Any generated Dart/TypeScript/Rust client built against the old route segment
for such a model will 404 against a server built with this fix, and vice versa, until both sides
are rebuilt together.

### CI, tooling, and internal fixes

`grpc/service.rs`'s five CRUD arm builders (`build_get_arm`/`build_delete_arm`/`build_create_arm`/
`build_update_arm`/`build_list_arm`) each independently reimplemented the same per-arm marker
struct, `UnaryService` impl, and `CoolError`-to-`tonic::Status` mapping, differing only in which
dispatch fn to call and how many arguments to thread through. Deduplicated into a shared
`build_unary_arm(ArmSpec)` helper; generated output is unchanged — verified byte-identical via
`cargo expand` before and after, including a paged-list and a create-disallowed fixture the
existing test suite didn't otherwise exercise (#524).

CI now actually runs `cratestack-pg/tests/decimal_bigdecimal_backend.rs`, the live-Postgres
`decimal-bigdecimal` round-trip test #495/#496 added but never wired into a job: `tests-db` only
built `cratestack-pg` under its default feature set, and `.ci/feature-matrix.sh` only ever ran
`cargo check`, never `cargo test`, so a runtime regression in that codec/bind path would have
passed every existing gate silently (#520).

`just regen-examples` regenerates the two committed generated example clients
(`examples/flutter-riverpod/client`, `examples/react-vite-swr/client`) locally, reusing the exact
generator invocations `ci.yml`'s drift-check steps run so the recipe and CI can't copy-paste
diverge. Both example CI jobs' downstream steps (`cargo test`, `flutter analyze`/`test`, `pnpm
install`/`tsc`) now run and report their own pass/fail even when the drift check itself fails,
instead of being silently skipped behind it (#508).

The release-bump commit's `git add` staged the root `Cargo.lock` by name only, silently dropping
the four other lockfiles `just bump` also refreshes on disk (`crates/cratestack-studio-ui/Cargo.lock`
and the three standalone `examples/*-verification*/Cargo.lock` files, each its own `[workspace]`
root) — the exact gap that broke v0.7.10's own release PR (`facade-disjointness` failed with
`--locked` because one of those lockfiles was stale). Now staged via a glob pattern, closing the
gap structurally rather than chasing individual filenames (#503).

## 0.7.10 (2026-08-09)

### Per-call-site `ON CONFLICT DO NOTHING` for idempotent inserts (#487, ADR 0038 blocker B3)

`.upsert(..).run(..)` only ever emitted `INSERT ... ON CONFLICT DO UPDATE`. A model with any `upsert_update_columns` had no way to express "insert, or read back without mutating" — the fallback to a no-op `pk = EXCLUDED.pk` self-assignment only kicked in when `descriptor.upsert_update_columns` was empty, a property of the *model*, not the call site. Concretely: a cash-in claim inserting a `PENDING` row and treating a unique violation as "already in flight" would have a retry's blank values silently overwrite an existing `COMPLETED` row's `transfer_ref`/`new_balance_xaf`/`completed_at` — ledger corruption on retry, not a cosmetic gap. Consumers were hand-rolling `DO NOTHING RETURNING id` + a fallback `SELECT` to avoid exactly this.

`UpsertRecord` (from `cool.model().upsert(input)`) gains `.do_nothing()`, switching to a distinct builder (`UpsertRecordDoNothing`, mirrored for the `.bind(ctx)`-scoped delegate as `ScopedUpsertRecordDoNothing`) whose `.run()`/`.run_in_tx()` return `UpsertOutcome<M>` — `Inserted(M)` or `Existing(M)` — instead of a bare `M`, since a real `DO NOTHING` returns nothing on conflict and the caller needs "I inserted this" distinguishable from "this already existed and I left it alone." This is additive: `.upsert(..).run(..)` without `.do_nothing()` keeps its `Result<M, CoolError>` signature and DO UPDATE semantics unchanged.

Race semantics, spelled out in `UpsertOutcome`'s doc comment: the runtime always resolves the conflict target under the same `SELECT ... FOR UPDATE` row lock the DO UPDATE path already uses. If the probe finds an existing row, that lock guarantees it's still there at commit, so `Existing` is returned directly with no second statement — DO NOTHING genuinely never touches the row (no trigger fan-out, no `xmax` bump, no WAL), unlike the DO UPDATE path's no-op self-assignment fallback, which is a real (if degenerate) write. If the probe finds nothing, the actual `INSERT ... ON CONFLICT DO NOTHING RETURNING` still runs (not a plain `INSERT`) because "no row" from a `SELECT` doesn't lock anything — a concurrent transaction can still win the race. On that loss, the runtime performs one more locked read to hand back the row the other transaction actually committed; if *that* row is deleted before the fallback read completes (a second, narrower race), it surfaces `CoolError::Conflict` rather than inventing a result, and the caller retries.

The existing empty-`upsert_update_columns` no-op-self-assignment fallback in the plain DO UPDATE path is kept, deliberately not merged into `.do_nothing()`: it exists to make `RETURNING` resolve for a *model* shape (zero eligible update columns), while `.do_nothing()` is an explicit *per-call* opt-in independent of that shape, and the two have different storage-layer effects (no-op `DO UPDATE` still fires triggers/bumps `xmax`; genuine `DO NOTHING` doesn't). Merging them would either force every empty-`upsert_update_columns` model onto DO-NOTHING semantics for existing callers or make `.do_nothing()` pay for trigger fan-out it explicitly asked to avoid.

Scoped to `cratestack-sqlx` (Postgres) only. `cratestack-rusqlite` has an equivalent `INSERT ... ON CONFLICT DO UPDATE` upsert path (`render_upsert_with_conflict`) and SQLite supports `DO NOTHING RETURNING` too, but that backend's upsert is a single statement with no pre-probe (no policy/audit/event machinery to preserve, but also no existing "inserted vs. existing" discriminator to build on) — giving it the same capability is a materially different, smaller design left as follow-up rather than folded in here.

New PG-backed regression coverage in `crates/cratestack-pg/tests/upsert_do_nothing.rs`: a ledger-corruption reproduction (insert with real values, retry via `.do_nothing()` with blank values, assert the row is byte-for-byte unmodified — confirmed failing on `main`/028cdc5 via `.upsert().run()`, the only pre-#487 API, before this change existed to fix it), an `Inserted`-vs-`Existing` distinguishability test with audit/event-outbox assertions (no `Updated` event or audit row on the `Existing` branch), a same-fixture regression test that the plain DO UPDATE path is unaffected, and an empty-`upsert_update_columns` model case.

### Generated Dart and TypeScript clients get a real `Decimal` type (#498) — breaking

`cratestack-client-dart` and `cratestack-client-typescript` used to carry every `Decimal`-typed field as an opaque wire-format string — harmless for the default `decimal-rust-decimal` backend (which never emits scientific notation), but silently wrong once #495/#496 made `decimal-bigdecimal` (arbitrary precision, beyond `rust_decimal`'s ~28-29 significant-digit cap) a real, selectable server backend: `bigdecimal`'s `Display` switches to scientific notation past a magnitude threshold (`"0.0000001"` on `rust_decimal`, `"1E-7"` on `bigdecimal`, for the identical value), so the *string form* a Dart/TS client saw depended on which backend built the server, and neither SDK could parse, compare, or do arithmetic on the value at all — the exact case #495/#496's own PR flagged as unfinished business.

The maintainer's recorded decision (of the three approaches priced in #498's own ticket — wire canonicalization, a real client-side decimal type, or refusing the combination) is the middle one: give the SDKs a real decimal type, not change what the wire carries.

- **Dart**: `Decimal`-typed fields (including `DecimalFilter`'s comparison operands) are now `package:decimal`'s `Decimal` class, not `String`. `wire_decode.rs`/`wire_encode.rs` decode via `Decimal.parse` (accepts both plain and scientific notation into the identical value) and encode via `.toString()` (always plain positional notation, matching `rust_decimal`'s own `Display`). Every generated `pubspec.yaml` (default, riverpod, and gRPC presets — gRPC reuses the same generated model classes) gains a `decimal: ^3.2.6` dependency. Because Dart's `Model.fromWire`/`.toWire()` factories are the single decode/encode chokepoint every transport (REST, RPC, and gRPC's own message registry — `grpc_runtime/decode.dart.j2`'s `decodeMessage` hands a plain `Map` to the exact same `fromWire`) already routes through, this is a complete fix: gRPC's proto3 wire type stays `string` (unchanged, see below) but the in-memory value on every transport is a real `Decimal`.
- **TypeScript**: `Decimal`-typed fields are now `decimal.js`'s `Decimal` class (re-exported from the generated `models.ts` as a `DecimalJs.clone({ toExpNeg: -1e9, toExpPos: 1e9 })` — an unbounded-exponent clone, so `.toString()`/`.toJSON()` always emit plain positional notation too, for the same reason as Dart's `.toString()` choice). `decimal.js` is a `dependencies` entry (not `peerDependencies`, unlike `@tanstack/react-query`) in every generated `package.json` — every consumer needs a working `Decimal` implementation and there's no app-owned-singleton constraint the way there is for React/react-query, so nothing is gained by pushing the choice onto the app. Adds ~32 KB minified / ~13 KB minified+gzip, zero transitive dependencies (measured: `npm view decimal.js dist.unpackedSize` reports 284 KB unpacked; a local `terser` minify of the runtime file alone is 32,328 bytes, 12,860 gzipped).

  Unlike Dart, this package had **no decode/encode chokepoint at all** before this change — every response was a blind `JSON.parse`/codec-decode cast with `as T`, no runtime transform of any kind (a `DateTime` field is still a bare `string`, by design). A real `Decimal` instance needs an actual runtime replace of the wire string, so `models.ts.j2` gains a `decimalShapes` registry (one `DecimalShape { keys, nested }` entry per model/`type` the schema declares — `crate::decimal::build_decimal_shapes`), a `reviveDecimalFields(value, shapeName)`/`revivePagedDecimalFields(value, shapeName)` pair keyed by that registry, and a `reviveDecimalScalar(value)` counterpart for a return value that is itself `Decimal` (not wrapped in an object). Every generated decode call site — the `default`-preset REST (`rest-client.ts.j2`) and RPC (`rpc-client.ts.j2`) clients' `list`/`get`/`create`/`update` methods **and their `ProceduresApi`**, the `swr` preset's per-model functions (`models-rest.ts.j2`/`models-rpc.ts.j2`) **and its `procedures.ts`** — calls one of these unconditionally, keyed by the decoded type's own registry entry name (a name with no entry, e.g. a plain scalar/enum return, is a documented fast-path no-op, so this is a uniform, always-present `.then(...)` wrapper rather than the generator branching per call site). A relation-embedded `Decimal` field (a `Post.author.balance` shape) revives too, not just a flat field on the root — `reviveShaped` routes a nested field to *its own* type's shape via `nested`, recursively. `Decimal.prototype.toJSON` (an alias for `.toString()`) makes the *encode* direction (`JSON.stringify`, which both a REST request body and this package's default `jsonRpcCodec` go through) work automatically, with no generated glue needed. `DecimalFilter`'s comparison operands are `ComparableFilter<Decimal>` now; they never need decode-side revival since a `Where`/`FindMany` argument only ever travels outbound.

  **A real bug was caught and fixed by a second reviewer before this landed, not just theorized:** the first version of this scheme (`crate::decimal::DecimalReachability`) kept a single flat `Set<string>` of every `Decimal` field name reachable from a response's root type — its own fields *and* every relation's/`type`'s fields, unioned together — and matched it against a decoded response's keys at *any* nesting depth. That is unsound the moment two *different* reachable types can each contribute a field name to the same flat set: an `Order.total: Decimal` + related `Account.total: String` schema, `include`-ing the relation, either threw `[DecimalError] Invalid argument]` decoding a real (non-numeric) account reference or silently corrupted a numeric-looking one (`"00123"` -> `Decimal("123")`, losing its leading zeros) — reproduced empirically (`tests/fixtures/decimal_name_collision.cstack`, `tests/decimal_collision_regression.rs`), not just reasoned about. Replaced with the path-aware `decimalShapes` registry above: every type keeps its own `Decimal` field names in its own shape, never merged with another type's, so `Account.total` is only ever checked against *Account's* shape (which correctly has no `total` key).

- **gRPC (both SDKs):** `grpc/wire.rs` still maps `Decimal` to proto3 `string` on the wire (unchanged, matching `cratestack-proto::emit::scalar::map_scalar`). Dart's gRPC preset reuses the identical `Model.fromWire`/`.toWire()` factories the REST/RPC presets use (confirmed by generating a `transport grpc` schema with a `Decimal` field and running `flutter analyze`/`flutter test` — clean, including a real relation-embedded-field and procedure-return-type round trip), so it's not just non-broken, it's *correct*: proto3 carries a string, the in-memory value is a real `Decimal` — and, since Dart's decode always goes through a real per-field-typed `fromWire`, never a flat name-keyed set, it was never exposed to the collision class above either. TypeScript's gRPC-Web preset gains a dedicated `"decimal"` `GrpcWireKind` (`wire.rs`, `grpc-web-runtime.ts.j2`'s `encodeScalar`/`decodeScalar`/`zeroValue`) — same proto3 `string` wire bytes, decoded into a real `Decimal` (imported from `./models.js`) rather than the raw JS `string` an earlier draft of this change left it as; a probe schema (`transport grpc`, a `Decimal` field) both `tsc`-typechecks and round-trips a scientific-notation value through the real generated `encodeMessage`/`decodeMessage`, proven by a real `npx vitest run`. gRPC's own decode (`decodeMessage`) is per-message-type-scoped by construction (field descriptors are looked up per message, never name-matched across types), so it was never exposed to the flat-key collision class either.

**Breaking, on the default `decimal-rust-decimal` backend, whether or not a schema uses `decimal-bigdecimal` at all:** existing app code doing `model.amountField` and expecting a `string`/`String` now gets a `Decimal`/`decimal.js` `Decimal` instance instead. Migration: replace `String`/string-typed usage of a `Decimal` field with the respective library's API — Dart: `Decimal.parse(input)` to construct, `.toString()` to format, comparison operators/`compareTo` work directly; TypeScript: `new Decimal(input)`, `.toString()`, `.plus()`/`.minus()`/`.cmp()` etc. instead of raw arithmetic/string comparison. `DecimalFilter`'s `eq`/`ne`/`lt`/`lte`/`gt`/`gte`/`in` fields need the same treatment when constructing a `Where`/`FindMany` argument by hand.

Verified: `cargo test -p cratestack-client-dart -p cratestack-client-typescript` (generator/snapshot suites, all fixtures reviewed line by line, not just re-recorded); two new real-toolchain round-trip tests — `cratestack-client-dart/tests/decimal_round_trip.rs` (`flutter pub get` + `flutter test` against a generated package) and `cratestack-client-typescript/tests/decimal_round_trip.rs` (`npm install` + `npx vitest run`) — proving a value beyond `rust_decimal`'s capacity, in both plain and scientific notation, round-trips through the real generated `fromWire`/`toWire` (Dart) and REST client (TypeScript) with its value intact; `just verify-dart`/`just verify-typescript` (generation + `flutter analyze`/`tsc` against the `ci_rest`/`ci_rpc`/riverpod fixtures, none of which declare a `Decimal` field, so this also proves the `DecimalFilter`-only boilerplate change doesn't regress every existing generated package).

### `auth().isSystem()` — a system principal for server-internal reads and writes (#486, ADR 0038 blocker B1)

Model policies can now name a trusted system principal instead of only end-user claims: `@@allow("update", auth().isSystem() || subjectId == auth().subjectId)`. This is the half of #486's proposal being shipped now — the spike in #485 (closed, do-not-merge) also prototyped `@@internal(...)` route suppression, but that covers REST only and is a false guarantee under `transport rpc` (a "suppressed" route would still be reachable over RPC); the spike's own recommendation, followed here, was to ship `isSystem()` first since it alone unblocks the read-side problem route suppression does nothing for. `@@internal` is **not** part of this change.

Today, giving server code (procedures, workers, reconciliation jobs) a way to write through the ORM means adding an `@@allow("update", ...)` policy, which also opens a public CRUD route for that action — and an owner-scoped `@@allow("detail", subjectId == auth().subjectId)` denies a legitimate internal read, since a service caller carries no subject claim. Both push consumers toward hand-written raw SQL instead of the generated surface.

`isSystem()` is a term a policy **names**, not a bypass flag: `db.model().unchecked().update(...)` was explicitly rejected because it would move authorization out of the schema into scattered call sites, with nothing distinguishing a legitimate escalation from an accidental one. `cratestack_core::SystemContext::for_service("...")` is the only way to obtain a `CoolContext` that satisfies `auth().isSystem()`; it has no `From`/`TryFrom<CoolContext>` and no constructor accepting an existing (e.g. request-derived) context, and the backing flag is a private, `#[serde(skip)]` field on `CoolContext` — so no `AuthProvider` implementation, and no deserialized/wire-carried context, can ever produce one. **Fail-closed:** a model whose policies never write `isSystem()` gains nothing from a system caller — the predicate only ever satisfies a clause a schema author wrote down, proven by `model_that_never_names_is_system_denies_system_callers` (`cratestack-sqlx`) and `system_caller_is_denied_on_a_model_that_never_names_is_system` (PG-backed, `cratestack-pg/tests/system_principal.rs`). **Auditable:** `SystemContext::for_service` records the service name as both the `id` (`system:<service>`) and `service` claims, which flow unchanged into the existing `cratestack_audit` actor — no new audit machinery needed, see `system_write_is_captured_in_the_audit_trail`.

Wired through all three places a model read policy is evaluated: the create-path in-process evaluator (`cratestack-sqlx::query::support::create::evaluate_input_predicate`), the `QueryBuilder` pushdown used by row-scoped write authorization (`query::support::policy_predicate::push_policy_predicate`), and the SQL-string renderer used by `find_unique`/list reads (`render::policy_predicate::render_policy_predicate`) — this last one is why PG-backed coverage exists alongside the unit tests: it's the read path route suppression could never have fixed. `auth().isSystem()` is recognised in the model policy-term parser (`cratestack-macros::policy::model::term`) ahead of the generic builtin-call parser, which would otherwise misparse `auth()`'s own parens as the function call.

Verified: `cargo test -p cratestack-core -p cratestack-policy -p cratestack-macros -p cratestack-sqlx`, plus `cratestack-pg/tests/system_principal.rs` against a real Postgres (`just test-pg-only system_principal`), covering all five acceptance criteria — system-permitted where named (read and write), fail-closed where not named, non-system callers unaffected, audit capture, and an HTTP-request forgery attempt (a plausible "naively forward a client claim" `AuthProvider` bug) that still cannot produce a system context.

### A real `decimal-bigdecimal` backend (#495) (#496) — breaking

`decimal-bigdecimal` was removed in #464/cfde4e0 for being a dead `compile_error!` — declared but never implemented. This implements it for real: `cratestack-core::Decimal` is now `cfg`-gated per backend (`rust_decimal::Decimal` under `decimal-rust-decimal`, `bigdecimal::BigDecimal` under `decimal-bigdecimal`), with two `compile_error!`s enforcing that exactly one is selected — neither (the pre-existing check) and now also both (new: the two are mutually exclusive, so `--all-features` trips it again, this time for a real reason).

The two backends are not drop-in equivalents: `rust_decimal::Decimal` is `Copy`; `bigdecimal::BigDecimal` heap-allocates and is not. A workspace-wide audit for double-moves/implicit-copy reliance (source and tests) found exactly one real call site — `cratestack-sqlx`'s `push_bind_value` dereferenced a `&Decimal` to bind it (`*value`), which only compiles for a `Copy` type — fixed with `.clone()`, which degrades to a cheap bitwise copy under `decimal-rust-decimal` and a real allocation under `decimal-bigdecimal`. No `derive(Copy)` on any `Decimal`-carrying type exists anywhere in the workspace. Both backends implement `Clone`/`Debug`/`Display`/`FromStr`/`PartialEq`/`PartialOrd`/`Ord`/`Eq`/`Hash`/`Default`, so no other trait bound needed to change.

Making the swap reachable, not just possible in `cratestack-core` alone, meant widening #421's "one shared `default-features = false` dependency edge" pattern to the full transitive closure between a facade and `cratestack-core`: `cratestack-sql`, `cratestack-policy`, `cratestack-parser`, `cratestack-proto`, `cratestack-macros`, `cratestack-axum`, `cratestack-codec-cbor`, and `cratestack-codec-json` all gained the same `default-features = false` (at the workspace-dependency site) plus explicit per-consumer `decimal-rust-decimal`/`decimal-bigdecimal` forwards that `cratestack-core`/`cratestack-sqlx`/`cratestack-rusqlite`/`cratestack-client-rust` already had — a single crate left pinning `decimal-rust-decimal` anywhere in that closure re-forces it for the whole graph, since Cargo features are additive and unify globally. `cratestack-sqlx`'s `decimal-bigdecimal` feature forwards to `sqlx-core`/`sqlx-postgres`'s own (implicit, un-gated-by-name) `bigdecimal` features, giving `cratestack_core::Decimal` real `sqlx::Type`/`Encode`/`Decode` impls against Postgres `NUMERIC` under either backend through the exact same integration points (`push_bind_value`, generated `row.try_get(...)`) — neither of which names a concrete backend type.

`cratestack-pg`'s `postgres` feature previously forwarded `cratestack-sqlx/decimal-rust-decimal` unconditionally (the #421 fix, reasonable when there was only one backend to want). That's now removed: forcing a specific backend there would make `--features postgres,decimal-bigdecimal` request both backends on `cratestack-sqlx` simultaneously, hitting the new mutual-exclusion `compile_error!` — exactly the outcome this issue exists to avoid. `postgres` alone (no explicit decimal feature) is consequently a deliberate compile failure now; `.ci/feature-matrix.sh` asserts this explicitly so a future "fix" that silently re-adds the force gets caught.

**Breaking:** any consumer of `cratestack-pg`, `cratestack-api`, `cratestack-sqlite`, or `cratestack-client` using `default-features = false` must now explicitly select `decimal-rust-decimal` or `decimal-bigdecimal` — a bare `--no-default-features` (or `default-features = false` with no re-added decimal feature) that used to silently resolve to `rust_decimal` is now a `compile_error!`. `examples/no-database-verification` hit exactly this (`cratestack-pg` with `default-features = false`, the configuration `crates/cratestack-pg/README.md` documents) and needed an explicit `features = ["decimal-rust-decimal"]` re-add — see that example's own `Cargo.toml` comment for why the *host*-side `cratestack-core` (compiled in for the `cratestack-macros` proc-macro) has no other path to a backend even when a target-side dependency happens to request one.

One structural limitation found and left as an intentional workaround, not a design choice: `cratestack-macros/tests/ui.rs`'s `trybuild`-based compile-fail suite generates a synthetic crate that copies `cratestack-macros`' entire `[dependencies]` table (including `cratestack-core`) onto its own dependency list — separate from the proc-macro's own resolution of the same package — and `trybuild`'s feature-copying logic only preserves `dep:xxx` weak-optional-dependency forwards, silently dropping ordinary `"pkg/feature"` strings. The copied `cratestack-core` edge therefore never received a decimal-backend forward and hit the "neither selected" `compile_error!` for a test unrelated to decimals at all. Fixed by adding `resolver = "1"` to `cratestack-macros`' own `[package]` — genuinely ignored by Cargo for build purposes on this crate as a real workspace member (the root `[workspace] resolver = "3"` governs there regardless; only `trybuild`'s standalone synthetic mini-workspace obeys it, reuniting the two copies' feature resolution the way they did before this backend existed), but **not silent**: it emits `warning: resolver for the non root package will be ignored` on every `cargo check`/`build`/`test` invocation anywhere in the workspace (verified: even `cargo check -p cratestack-core`, an unrelated crate, prints it), because Cargo evaluates the full workspace manifest tree regardless of which package is targeted. That tradeoff — permanent, harmless build noise vs. a broken `trybuild` suite, given `trybuild` has no configuration hook to avoid this and hardcoding the decimal feature on the edge instead would re-open the exact leak #495 closes — is accepted for now; see `cratestack-macros/Cargo.toml`'s own comment for the alternatives considered and rejected.

`cratestack-cli`'s four remaining tool-crate dependencies (`cratestack-studio`, `cratestack-mock-wiremock`, `cratestack-client-dart`, `cratestack-client-typescript`) were plain `.workspace = true` edges with no `default-features = false`, so their own `decimal-rust-decimal` default stayed force-enabled regardless of what `cratestack-cli` itself requested — `cargo check -p cratestack-cli --no-default-features --features decimal-bigdecimal` hard-failed with a `compile_error!` that pointed nowhere near the real cause. Fixed for #496 by widening the same `default-features = false` + explicit-forward treatment to those four edges too; `cratestack-cli` now fully displaces `rust_decimal` under `decimal-bigdecimal`, closing the gap the original #495 PR left as out-of-scope.

**Cross-backend wire compatibility constraint:** ordinary `Decimal` values encode identically on the wire (CBOR and JSON) under either backend, but `bigdecimal` emits scientific notation (e.g. `"1E-29"`) for values past `rust_decimal`'s ~28-29 significant-digit capacity, which a `rust_decimal` peer cannot decode. Since the shipped Dart and TypeScript client SDKs only ever target the default (`rust_decimal`) backend, a `decimal-bigdecimal` server cannot safely use its extra precision when talking to them — see `crates/cratestack-core/README.md` and the facades' feature docs for the full deployment constraint.

Verified: `cargo check -p cratestack-pg --no-default-features --features postgres,decimal-bigdecimal`, `cargo check -p cratestack-client --no-default-features --features decimal-bigdecimal`, `cargo check -p cratestack-cli --no-default-features --features decimal-bigdecimal`, and `cargo tree -p cratestack-client --no-default-features --features decimal-bigdecimal -e features | grep rust_decimal` (prints nothing) all pass, plus a new live-Postgres round-trip test (`cratestack-pg/tests/decimal_bigdecimal_backend.rs`, `required-features = ["postgres", "decimal-bigdecimal"]`, mirrors `pgvector_feature_forwarding.rs`'s pattern) confirming both an in-range and a beyond-`rust_decimal`-capacity `Decimal` field round-trip through `NUMERIC` under the new backend without precision loss.

### A fourth facade, `cratestack-client`, for pure HTTP-client SDK crates (#490)

`include_client_schema!` was previously only reachable through `cratestack-pg`, `cratestack-api`, or `cratestack-sqlite` — all three of which carry `cratestack-axum` (and therefore `axum`/`tower`/`hyper`/`tower-http`, a full server framework) unconditionally, even when a consumer only ever calls a cratestack server and never runs one. `cratestack-client` re-exports **only** `include_client_schema!` (not the other two entry macros — reaching for either now fails with a plain name-resolution error) plus the generated Rust client runtime and the handful of type re-exports client codegen actually references, derived empirically by tracing every `::cratestack::` path `include_client_schema!`'s expansion can emit. `cratestack-axum` is structurally absent from its dependency graph under default features — proved by a new standalone verification workspace, `examples/client-only-verification` (mirrors `examples/no-database-verification-api`'s cratestack#347 precedent: its own `[workspace]` root with a committed `Cargo.lock`, not a member of the root workspace, since Cargo unifies features across workspace members). This facade has no `grpc` Cargo feature: `cratestack-client-rust`'s own `grpc` feature pulls `tonic`, which pulls `axum` transitively, defeating the point; a gRPC-client consumer should depend on `cratestack-client-rust` directly with `features = ["grpc"]` instead.

Building the empirical re-export list surfaced a real, pre-existing gap: `RpcListInput`/`RpcPkInput`/`RpcUpdateInput`/`RpcListPredicate` (the RPC model-CRUD input envelopes `transport rpc` client codegen references as `::cratestack::rpc::*`) were defined in `cratestack-axum::rpc::inputs`, not `cratestack-core::rpc` alongside their sibling wire shapes (`RpcErrorBody`, `RpcRequest`, `RpcResponseFrame`) — an oversight relative to that module's own stated goal ("clients can depend on a single source of truth without pulling in axum"), invisible until a facade without `cratestack-axum` in its graph tried to compile a `transport rpc` schema with model CRUD. The four types move to `cratestack-core::rpc`, with `cratestack-axum::rpc` re-exporting them unchanged — same names, same shapes, same wire format, so `cratestack-pg`/`cratestack-api`/`cratestack-sqlite` see no behavior change.

The facade also declares `pgvector` and `rate_limit` Cargo features. `include_client_schema!` runs the same extension-declaration gate as the server and embedded macros, so without them a schema containing `extension pgvector { }` or `extension rate_limit { }` was a hard `compile_error!` through this facade with no feature to opt into — which, since a client SDK is generated from the same `.cstack` the server is built from, ruled out every server schema using embeddings or rate limiting. Unlike `cratestack-pg`'s same-named features these forward to `cratestack-macros` alone, with no runtime half: a `Vector(n)` field reaches the generated client as a plain `Vec<f32>` (the `pgvector` crate is involved only at the server's sqlx row-decode boundary) and `@no_rate_limit` only affects enforcement living in `cratestack-axum`. They are schema-compatibility switches, not feature implementations.
### `cratestack-axum` response content-type negotiation stops picking codecs the router can't actually encode (#489)

A router built with a single codec (e.g. `router(db, procedures, JsonCodec, auth)`) returned a spurious `406 Not Acceptable` — `no encoder configured for response Content-Type application/cbor` — whenever a client's `Accept` header named `application/cbor` alongside `application/json`, even though the router had a perfectly good JSON encoder. Root cause: `RouteTransportCapabilities::response_types` is a compile-time list describing what the *transport shape* (REST/RPC binding) can carry across every possible codec configuration, not what the concrete `HttpTransport` a given router was actually constructed with can encode — `select_response_content_type` picked the first entry of that static list the client's `Accept` named, with no way to know the router only had one codec wired up.

`HttpTransport` gains a `can_encode(&self, content_type: &str) -> bool` method (defaulted to `true` — preserves the pre-#489 behavior for any downstream impl that hasn't opted in, since a required method would be a breaking change to this public trait), implemented honestly by both in-repo impls: the blanket `impl<C: CoolCodec> HttpTransport for C` and `CodecSet<Primary, Secondary>` (including the `application/cbor-seq` special case, encodable whenever *either* slot is a CBOR codec, regardless of position). Response negotiation (`select_transport_response_content_type`, used by both `encode_transport_result_with_status_for` and the sequence/`@stream` encoders) and the `Accept` preflight (`validate_transport_request_headers_for`/`validate_transport_response_headers_for`) now both filter the advertised `response_types` through `can_encode` before matching against `Accept` — the preflight fix additionally means a mutation like a model `create` now fails fast on an unsatisfiable `Accept` *before* its DB write runs, not only afterward when the response encoder finally catches it. A `NotAcceptable` the negotiator does return now names what the router actually serves, not the static list. One behavior change reaches slightly beyond the literal repro: a request carrying **no `Accept` header at all** previously got `default_response_type` unchecked, which for a JSON-only router is the codegen-baked `application/cbor` — the same 406 by another route. It now falls back to the first encodable type instead. Routers whose default is genuinely encodable (every dual-codec router, and any single-codec router whose codec matches the default) negotiate exactly as before. RPC unary/batch dispatch and gRPC bridging funnel through the same `encode_transport_result_with_status_for`/`RPC_BINDING_CAPABILITIES` path, so they're fixed by the same change; `validate_subscribe_accept_header` (SSE `@@subscribe`) was audited and left alone since it always produces `text/event-stream` unconditionally — there's no static-list-vs-codec gap there to begin with.

### `ClientStateStore` moves out of `cratestack-client-rust` into `cratestack-core` (#475) (#482) — breaking

`cratestack-client-store-sqlite` and `cratestack-client-store-redis` are storage adapters, but both depended on `cratestack-client-rust` — an HTTP transport binding — for the sole reason that `ClientStateStore` (plus `PersistedClientState`, `RequestJournalEntry`, `InMemoryStateStore`, `JsonFileStateStore`) happened to be defined there: an L2 → L4 back-edge, the client-side twin of the `cratestack-sqlx`/`cratestack-redis` → `cratestack-axum` edge #465 fixed server-side and the violation `docs/design/layering.md` named as still open. The trait and its companion types move to `cratestack-core::store::client_state`, with `cratestack-client-rust::state` kept as a back-compat re-export so existing `use cratestack_client_rust::state::...` paths keep compiling. `cratestack-client-store-redis` no longer depends on `cratestack-client-rust` at all; `cratestack-client-store-sqlite` keeps it only as a `[dev-dependencies]` entry for its test fixtures — `cargo tree -p cratestack-client-store-sqlite -i cratestack-client-rust` now reports a dev-dependency path only, and the same command for `-store-redis` reports no path at all. **Breaking:** anyone implementing `ClientStateStore` directly against `cratestack_client_rust::ClientStateStore` (rather than the re-export) needs to retarget `cratestack_core::ClientStateStore`; the trait's shape is unchanged.

`CratestackClient::state()` and the internal `record_request` journal-write path convert the moved trait's `CoolError` back to `ClientError::State(..)` explicitly at both call sites, rather than through `ClientError`'s blanket `From<CoolError>` (which targets `ClientError::Codec`, for genuine wire-codec failures) — an initial version of this move routed state-store failures through that blanket conversion, which would have silently reclassified local state-store I/O failures (a locked/corrupt JSON file, a poisoned mutex) as fabricated HTTP-500 `RuntimeErrorCode::Codec` errors instead of `RuntimeErrorCode::State`, reaching as far as the Dart/Flutter FFI boundary. Regression tests (`client::core::tests::state_store_error_maps_to_client_error_state`, `client::headers::tests::record_request_state_store_error_maps_to_client_error_state`) exercise a rigged-to-fail state store and assert the resulting `ClientError` variant.

### CI, release tooling, and process fixes

Layer direction (ADR 0014) is now CI-enforced: `docs/adr/layers.toml` assigns every `cratestack-*` crate a layer, and `.ci/layer-direction-check.sh` reads the real `cargo metadata` dependency graph and fails on any `cratestack-*` → `cratestack-*` edge pointing at a higher layer, or on a crate under `crates/` missing from the manifest (#477).

The release pipeline now actually writes a changelog. Nothing did before — `prepare-release.yml` already walked the commit range to build its release-PR body, but discarded that output rather than persisting it, which is why this file had been caught up by hand-written backfill PRs instead. `.ci/changelog-seed.sh` seeds a `## X.Y.Z` section per release (grouped by conventional-commit type, marked with a TODO placeholder), `.ci/changelog-check.sh` fails CI on an unedited marker so a seed can't silently reach `main` as a raw commit list instead of prose, and `just changelog-seed VERSION` runs it locally (#479/#483). Everything from `v0.5.0` through `v0.7.8` — thirteen releases — was itself backfilled by hand in a dedicated pass, since it had gone undocumented (#478).

`@no_rate_limit` reached the generated `OpDescriptor` (`rate_limited_by_default: false`) and was covered by tests at every layer except the one that mattered: `cratestack-axum`'s `RateLimitLayer` never read the flag, so an annotated procedure was still throttled at runtime regardless. Fixed with an ops-filter wired into both REST and RPC dispatch (#474/#481).

`rust-version = "1.95.0"` is now declared in `[workspace.package]`, matching the existing `rust-toolchain.toml` pin, with a dedicated `msrv` CI job building the workspace on that exact toolchain and a three-way drift check (resolved `rustc --version`, the toolchain file, and `Cargo.toml`'s declared `rust-version`) added to the existing `check` job (#422/#480).

`AGENTS.md` now records that this repo's own `cratestack-sqlx`/`cratestack-pg`/`cratestack-cli` dependencies on `sqlx` are a deliberate exemption from the no-raw-SQL policy downstream consumers (webank-context ADR 0038) are adopting — this is the layer that wraps sqlx, not an instance of the drift that policy targets (#484).

`just bump`'s `cratestack-studio-ui` step changed directory with a bare `cd`, which leaked into every repo-root-relative path after it — including the standalone example-workspace lock-refresh steps `#422`/`#480` and later PRs added, which silently never ran as a result. Wrapped in a subshell like the steps around it (#494).

## 0.7.8 (2026-08-08)

### Rate-limit and idempotency layers stop trusting spoofable proxy headers (#416)

`cratestack-axum`'s idempotency and rate-limit layers previously fell back to a shared literal `"anonymous"` bucket whenever a request carried no `Authorization` header — weak, but at least not attacker-steerable. A first attempt at improving this replaced the fallback with a client-IP parsed from the `Forwarded`/`X-Forwarded-For` headers, which turned out to be worse: the crate has no trusted-proxy configuration to verify or strip those headers, so any caller reaching the service directly, or through a proxy that doesn't rewrite them, could mint a fresh rate-limit bucket per request or land in another caller's idempotency namespace just by setting an arbitrary header value.

The header-parsed fallback is replaced with axum's `ConnectInfo<SocketAddr>`, which reflects the actual accepted TCP socket and can't be spoofed by the client; when `ConnectInfo` isn't available the layers fall back to the original shared `"anonymous"` bucket rather than trusting an unverifiable header. This closes the header-spoofing hole, but not the underlying gap it was filed against (#416): no shipped example, including the flagship `server_basic.rs`, and no macro-generated wiring actually serves through `into_make_service_with_connect_info`, so in every default/documented deployment today, unauthenticated callers still collapse onto the shared bucket. #416 stays open; picking a config surface that guarantees `ConnectInfo` availability across every server-wiring path is left to the still-maintainer-blocked trusted-proxy design (#415).

### Storage traits move out of the HTTP crate; the layer model gets written down (#424, #472)

`IdempotencyStore`, `RateLimitStore`/`RateLimitConfig`/`RateLimitDecision`, and the idempotency-table DDL lived in `cratestack-axum`, which meant `cratestack-sqlx` and `cratestack-redis` depended on the HTTP transport crate solely to implement those traits — a back-edge against the intended `parser → core/policy/sql → macros → runtimes` direction. They move to `cratestack-core::store::{idempotency,ratelimit}` and `cratestack-sql::idempotency`, with re-exports kept in `cratestack-axum` for source compatibility; `cargo tree -i cratestack-axum` now returns no match from either `cratestack-sqlx` or `cratestack-redis` (#424).

A companion, docs-only change adds `docs/design/layering.md` and ADRs 0011–0016, naming six layers (L0 Schema IR through L5 Facades, plus the orthogonal compiler) and writing down the dependency-direction rule that `CLAUDE.md` previously expressed as a five-crate chain that no longer covers a thirty-crate workspace. Three ADRs are Accepted (the layer model itself, no IoC container, facade disjointness); three are Proposed, naming decisions still open for a maintainer call (#472).

### Feature graph: the default-features leak is closed, and the dead `decimal-bigdecimal` feature is gone (#421)

`cratestack-core` declared `default = ["decimal-rust-decimal"]`, but none of its ~27 internal dependency edges, nor the `cratestack-pg → cratestack-sqlx` / `cratestack-sqlite → cratestack-rusqlite` facade-to-runtime edges, set `default-features = false`, so that default was force-enabled workspace-wide regardless of what a consumer explicitly asked for. `default-features = false` is now set at the workspace-dependency site for `cratestack-core`, `cratestack-sqlx`, and `cratestack-rusqlite` (Cargo requires the override there, not per-member), with every plain `cratestack-core.workspace = true` edge re-enabling `decimal-rust-decimal` explicitly, since it's currently the only backend `cratestack-core` can compile with at all. Closing the leak also surfaced a real, previously-unreachable gap: `cratestack-sqlx`'s query-builder support code binds `cratestack_core::Decimal` unconditionally, so `cratestack-pg --no-default-features --features postgres` alone would have failed to compile without also forwarding `cratestack-sqlx/decimal-rust-decimal` — fixed in the same change.

Alongside this, the `decimal-bigdecimal` feature — reserved but never implemented, and an unconditional `compile_error!` if enabled — is removed rather than left as a no-op trap. A new `.ci/feature-matrix.sh`, wired into `just feature-matrix` and a CI job, checks every facade with its own decimal toggle (pg, sqlite, sql, sqlx, rusqlite, api, cli) under both its default and a narrowed `--no-default-features` selection, plus the wasm32-only backend paths. **Breaking** for anyone relying on the previous implicit default: a `--no-default-features` consumer of `cratestack-core`/`-sqlx`/`-rusqlite` must now request `decimal-rust-decimal` explicitly. This addresses part of #421 — removing rather than implementing an alternative backend means a consumer still can't select a non-default decimal backend, so the issue remains open.

### `cratestack-client-rust`: `reqwest::Error` no longer leaks through the public error type (#425) — breaking

`ClientError::Transport` previously wrapped `reqwest::Error` directly via `#[from]`, exposing a third-party error type in a public enum's match arm. `ClientError::Transport`/`RpcClientError::Transport` now wrap a new opaque `TransportError` instead, with `reqwest_error()`/`into_source()` accessors and `std::error::Error::source()` wired through so chain-walking still reaches the original `reqwest::Error`. `ClientError`, `RpcClientError`, and `OpKind` are now `#[non_exhaustive]`, so future variants don't break downstream exhaustive matches — `cratestack-client-flutter`'s conversion match needed a wildcard arm to keep compiling. `ExtensionKind` was deliberately left exhaustive: its own doc comment calls it "a closed list by design," and a first pass that added `#[non_exhaustive]` there was reverted after review, since it would have forced silent fallback arms into safety-critical internal matches (feature gating, DDL mapping).

### sqlx: unique-violation conflicts and the read-policy SQL contract

Single-row `create`/`update` operations that hit a unique-constraint violation returned a generic 500 instead of a 409 Conflict; fixed via a new `CoolError::ConflictTyped(DbErrorInfo)` variant that still carries the SQLSTATE and constraint name the existing `db_sqlstate()`/`db_constraint()` accessors depend on (#414). Separately, `render_read_policy_sql` now unconditionally wraps its output in a self-contained parenthesized group, matching the contract `push_action_policy_query` already committed to in 0.7.2 (#410) — described by its own commit as a latent-hazard fix rather than a live exploit, since both real call sites already wrapped defensively, but it closes the door on a future call site reintroducing an operator-precedence authorization bypass (#428).

### CI and test-infrastructure catch-up

A blocking `tests-redis` job now runs `cratestack-redis`'s test suite against a real Redis via testcontainers, mirroring the existing Postgres pattern with its own `CRATESTACK_REQUIRE_REDIS` guard (#418). CI also gained a `cargo check --target wasm32-unknown-unknown` step for `cratestack-sqlite`, a wasm32 build of the embedded-browser-vite example, and a `typescript-verify` job that generates and `tsc`-checks both REST and RPC TypeScript fixtures (#419).

`generated_routes_emit_tracing_events`, flaky enough to need a documented 3x CI retry, turned out to be a real bug: `init_tracing()` called `tracing::subscriber::set_default()`, which only installs a thread-local dispatcher on the one thread running the `std::sync::Once` closure, so every other worker thread spawned by `cargo test`'s multi-threaded harness fell back to `NoSubscriber` and silently dropped events. Switching to `set_global_default()` fixed it for real, and the CI retry loop is removed (#417). A trybuild fixture nominally testing malformed-policy diagnostics turned out to contain no `@@allow`/`@@deny` at all and is replaced with a genuinely malformed policy predicate (#420). Separately, the committed `examples/flutter-riverpod` client was regenerated after its templates changed in 0.7.5 but the fixture itself hadn't been, leaving `generate-dart --check` red on `main` since 2026-08-06 (#470).

### Docs: proposals for four decision-blocked issues (#469)

One design note each for #413, #415, #422, and #426 — confirmed defects that can't be implemented until a maintainer makes a call an agent has no standing to make. Docs only, no code changes.

## 0.7.7 (2026-08-08)

### `RequestAuthorizer::authorize` becomes async (#453) (#454) — breaking

`cratestack-client-rust`'s `RequestAuthorizer` trait had a synchronous `authorize` method, unusable for a real credential provider — an OAuth2 client-credentials token with a refresh-on-expiry cache, for instance, needs an HTTP call on a cache miss. The only workarounds were `block_on` (panics or deadlocks depending on the runtime) or pre-fetching and stashing a token, which reintroduces the expiry race the cache existed to avoid.

`authorize` is now `async fn`, via `#[async_trait]` rather than a bare AFIT, because both `CratestackClient::with_request_authorizer` and `CratestackGrpcClient::with_request_authorizer` store the authorizer behind `Arc<dyn RequestAuthorizer>` and native AFIT isn't object-safe — the same shape `cratestack_core::audit::AuditSink` already uses. **Breaking:** every implementor must change `authorize` to `async fn` and add `#[async_trait::async_trait]` to the impl block. This release updates every in-workspace implementor and the README's sample impl; external code implementing the trait needs the same change.

### TypeScript client: `Decimal` model fields now generate valid TypeScript (#456) (#455)

`ts_type()` in `cratestack-client-typescript` had no `Decimal` arm, so a model field typed `Decimal` fell through to the catch-all and was emitted verbatim as a TypeScript type name nothing declares — generation reported success, and the failure only surfaced later at `tsc` as `TS2304: Cannot find name 'Decimal'`, once per field. Fixed by mapping `Decimal` to `string`, matching the two sibling call sites already in the same crate. The new regression test asserts the emitted annotation itself and was verified against a consumer schema with three `Decimal` fields, which now passes `tsc --noEmit` with zero `TS2304` where it previously failed with six.

### Docs correction: half-landed-release recovery advice

The recovery guidance added in 0.7.6 (#450) claimed a half-landed release could be recovered by fixing the cause and re-running the failed jobs against the same tag. That's only true for a transient failure: every publish job checks out the release tag, so a fix merged to `main` afterward is absent from a re-run, and `workflow_dispatch` only rebuilds binaries — it never touches crates.io or npm. Recovering v0.7.5 hit exactly this, and was instead recovered by releasing v0.7.6; `docs/tooling/npm-publishing.md` now says so (#452).

## 0.7.6 (2026-08-07)

### Model responses no longer round-trip through `serde_json::Value` before the wire codec (#430, #449)

Every list/detail response row was projected through `serde_json::to_value` before the real wire codec (`JsonCodec`/`CborCodec`) touched it. `serde_json::Value` always reports itself as human-readable, so any field whose `Serialize` impl branches on that hint took the human-readable path unconditionally — for `Uuid`, that meant the generated Rust client's `Uuid::deserialize` ran its bytes-branch against a text string under the default CBOR wire format and failed on every model with a `Uuid` column. This was the reason `policy_db.rs::db_backed_policy_enforcement` had been `#[ignore]`d.

The fix introduces `cratestack_axum::ProjectedValue`, a format-preserving intermediate that keeps each scalar leaf behind a type-erased `erased_serde::Serialize` object instead of pre-serializing to JSON, deferring the human-readable decision to the actual target serializer chosen per request via content negotiation. Its `Null` variant calls `serialize_none()` directly, which also retires a documented workaround that stripped null map entries to dodge a separate `minicbor-serde` quirk — that old workaround had never been applied to nullable to-one relation `include`s, so this incidentally fixes a second, latent CBOR-null bug there too. Landing this also surfaced several of `db_backed_policy_enforcement`'s own latent bugs (id-reuse across seeded rows, unspecified tie-break ordering, a stale expectation, a wrong status code); those are fixed and the test now runs for real in CI.

### Required auth fields can no longer silently resolve to NULL (#431, #448)

A `@default(auth().field)` backed by a *required* field in the schema's `auth` block was, on a missing value in the actual auth context, silently written as NULL rather than rejected — a real policy bypass for tenant-scoping fields, since SQL's `NULL != X` evaluates to NULL, not true. `resolve_default_value()` now tracks the auth block's declared arity for the field via a new `auth_field_required` flag, and returns `CoolError::Validation` when a required auth field is absent, before policy evaluation runs. A follow-up commit fixed a regression the initial version introduced — the required-field check had jumped ahead of the existing anonymous-caller check, turning an unauthenticated request's expected 403 into a 422 — and adds no-DB unit coverage of all branches.

### Parser rejects `type`/`enum`/`model` names that collide once normalized (#429, #447)

Declarations of different kinds whose names collided only after `to_snake_case` normalization were previously accepted silently, even though `type`/`enum` land in the same generated `types` module and `model` in a `models` module, both re-exported at the parent scope — a real collision there generates conflicting Rust symbols. The fix reuses the `find_snake_case_collision` helper from 0.7.2 (#408) to reject the three kind-pairs that actually share generated symbols. A follow-up commit narrowed an over-eager first pass that also rejected `mixin` and `auth` against every other kind: neither a mixin's own name nor an auth block's name is ever emitted as a generated identifier, so reuse there is legitimate.

### CI: idempotent npm publishes, pinned `wasm-opt` for releases (#450)

The v0.7.5 release run half-landed: crates.io and two npm packages published at 0.7.5 while every other npm package was stranded at 0.7.4, and re-running the workflow couldn't recover it. Two causes: npm's retry to Sigstore's Rekor log can race its own already-landed write and get back a 409 that `sigstore-js` surfaces as fatal rather than benign; and `wasm-pack` only downloads its own pinned `binaryen` when no `wasm-opt` is already on `PATH`, leaving an unpinned network fetch on the release build's critical path. Neither publish job had re-run tolerance either — a bare `npm publish` fails outright on an already-published version. All five `npm publish` call sites now route through `.github/scripts/npm-publish.sh`, which retries the Sigstore 409 with backoff and treats "already published" as success; both the release and CI wasm jobs now pre-install a pinned `binaryen` onto `PATH` ahead of `wasm-pack`.

### Docs: three facades documented, vestigial studio-generator shim removed (#427, #446)

`CLAUDE.md`'s facade section is updated from describing two facades to all three (`cratestack-pg`, `cratestack-api`, `cratestack-sqlite`), and `crates/cratestack-studio-generator` — a one-line re-export of `cratestack-studio::eject` that no workspace member depended on — is deleted, along with its references from the root `Cargo.toml`, `README.md`, and CI.

## 0.7.5 (2026-08-06)

### Dart Riverpod preset: fix `flutter analyze` failures on no-model and paged-first-model schemas (#443, #444)

`generate-dart --preset riverpod`'s generated `test/<package>_test.dart` imported `flutter_riverpod`/`flutter_test` unconditionally, but the only code using them was gated on `override_proof`, which is `None` whenever the schema has no models (a `provider = "none"` procedures-only service) or its first model in schema order is paged. For that legitimate schema shape both imports went unused, and the generated package's own lint config enables `unused_import`, so `flutter analyze` failed unconditionally. The fix gates the `flutter_riverpod` import the same way the RPC template's `fast_immutable_collections` import already was, and replaces the top-level bare `assert(...)` query-parameter checks with a real executed `test(...)` case. Confirmed against a real no-model service: `flutter analyze` went from 2 `unused_import` warnings to 0. None of the existing riverpod-preset snapshot fixtures exercised this shape, which is how it went uncaught; the three affected snapshots were refreshed.

Workspace bumped to 0.7.5 (#445) — version-literal and lockfile updates only.

## 0.7.4 (2026-08-05)

### `cratestack-mock-wiremock`: WireMock stubs generated from schema procedures (#438, #439)

A new crate, `cratestack-mock-wiremock`, and a `cratestack generate-wiremock` CLI subcommand derive WireMock stub mappings directly from a `.cstack` schema's procedures, so integration/e2e tests can run against a mock backend whose wire contract cannot silently drift from the real one. v1 scope is deliberately narrow: happy-path stubs for `procedure`/`mutation procedure` under `transport rest`/`rpc`, matched on method and path only — model CRUD routes, `transport grpc`, error-case stubs, and auth emulation are deferred. The crate was validated end-to-end against a real 1900-line, 40-procedure schema, producing 40 correct mapping files and a clean `--check` rerun.

Two review findings landed before merge. The RPC-transport stub built its `urlPath` as `/rpc/<name>` instead of the actual `/rpc/procedure.<name>` the RPC dispatch generator emits — every RPC-transport stub would have silently never matched a real client's request. And the cycle guard for synthesizing stub payload values only checked for a direct repeat of the *same* type name, so a mutual cycle like `type A { b: B[] }` / `type B { a: A }` raised a false unbreakable-cycle error even though `{ "b": [] }` is a perfectly finite value. Both are fixed with regression tests.

### `cratestack-client-rust`: stop forcing `aws-lc-rs` onto every consumer (#440, #441)

0.7.3's reqwest dependency requested the `rustls` feature, which on reqwest 0.13 unconditionally selects `aws-lc-rs` as the TLS crypto provider. Because `cratestack-pg` depends on `cratestack-client-rust` unconditionally, this forced `aws-lc-rs` onto every workspace depending on `cratestack` at all — breaking a from-scratch musl/scratch build (`aws-lc-rs` needs a cross C toolchain; `ring` doesn't) and tripping any `cargo-deny` policy banning `aws-lc-rs`. The fix switches to reqwest's `rustls-no-provider` feature, which keeps the rustls-backed stack but drops the forced provider selection; because that feature panics at `Client::build()` time if no provider was installed, `CratestackClient::new` and Studio's `ApiSource::new` now install a `ring` fallback provider (idempotent, a no-op if a consumer already installed one). `cargo tree -i aws-lc-rs` now shows no match anywhere in the workspace.

Workspace bumped to 0.7.4 (#442) — no user-facing content beyond the version number.

## 0.7.3 (2026-08-05)

### `cratestack-client-rust`: unpin `reqwest` to 0.13, off the dead `rustls-tls` feature name (#435) (#436)

The workspace's `reqwest` entry requested `rustls-tls`, a 0.12-only feature name (0.13 renamed it to `rustls`/`rustls-no-provider`), so even though the bare version requirement looked 0.13-permissive, Cargo could only satisfy the edge with the newest 0.12.x release still carrying the old name. Any downstream workspace also depending on reqwest 0.13 directly ended up with two live, incompatible `reqwest` instances in one dependency graph — confirmed against a real downstream `Cargo.lock` — which silently defeated `CratestackClient::with_http_client`'s dependency-injection point, since a caller's 0.13-typed client didn't unify with this crate's 0.12-typed one.

The fix pins to `reqwest = "0.13"` with `rustls` (0.13's rename, which auto-installs the `aws-lc-rs` provider — later replaced in 0.7.4) plus the newly-required `query` feature, since 0.13 splits `RequestBuilder::query()` behind it and `cratestack-studio` calls it directly. Closes #435.

## 0.7.2 (2026-08-05)

### Extensions: a declarative surface for opt-in capabilities (epic #152 done)

`.cstack` schemas can now declare `extension rate_limit { }` / `extension pgvector { }` as a new top-level block, recorded on `Schema.declared_extensions` (#153). On its own this is declare-only, but it feeds a shared compile-time gate: all three entry macros check every declared extension against the compiling crate's own Cargo features and fail with a `compile_error!` naming the extension and the feature to enable, instead of silently doing nothing when declaration and feature disagree (#161). `include_embedded_schema!` also rejects `extension pgvector { }` unconditionally, since pgvector has no embedded equivalent.

`rate_limit` is the first extension built on that gate: a bare `@no_rate_limit` procedure attribute, valid only when the schema declares `extension rate_limit { }`, flips a procedure's `rate_limited_by_default` to `false` (#154) — deliberately narrower than the epic's own proposal, since `cratestack-axum`'s existing `RateLimitLayer`/`RateLimitConfig` stay unconditionally compiled, with no numeric config or store-selection changes.

### pgvector: vector columns, ANN indexes, and distance queries (#155, #156, #163)

`pgvector` goes from a declared name to a working scalar type across three phases. Phase 1 adds `Vector(n)` as a parametric scalar, emits `CREATE EXTENSION IF NOT EXISTS vector;` DDL, and wires `SqlValue::Vector`/`NullVector` through the sqlx encode/decode boundary behind a new `pgvector` Cargo feature; `include_embedded_schema!` rejects `Vector(n)` outright (#155). Phase 2 generalizes `@@index([...], using: ..., opclass: "...")` — a general-purpose model attribute, not pgvector-specific — so index DDL can request `ivfflat`/`hnsw` in place of the implicit btree, with existing `@unique`-derived indexes still rendering byte-identical DDL to before (#156). Finally, `FieldRef::distance_to(metric, query_vector)` gives `.asc()`/`.desc()` ordering and threshold filtering, with `VectorMetric::{L2,Cosine,InnerProduct}` mapping 1:1 to pgvector's operators (#163).

### Migration baselining: adopt an existing live database (epic #202 done)

`cratestack migrate` gains the ability to point at an already-running Postgres database and adopt it, closing the gap where `migrate diff` against a missing snapshot always diffed against an empty schema and emitted a full `CREATE TABLE` for tables that already existed. Phase A extracts the `Schema -> IR` projection step into a public `project()`/`Projections` seam, a pure refactor that Phase B plugs into (#203). Phase B adds `cratestack-migrate::introspect::postgres`, gated behind an opt-in `postgres-introspect` feature, which queries a live database's `information_schema`/`pg_catalog` state and produces the same `Projections` shape `project()` produces from a parsed schema — anything it can't map is reported as `UnmappedColumn` rather than guessed at (#204). Phase C wires both into `cratestack migrate baseline`: introspect, diff against the authored schema for a drift report (never a hard failure by default), write the introspected snapshot, and record a synthetic row in `cratestack_migrations` (#205).

**Breaking:** the migration snapshot format now stores `Projections` (the IR) instead of a `Schema`, bumping the on-disk snapshot format version from 1 to 2 — a baseline run has no `Schema` to write, and a drifted database's snapshot needs to reflect live reality rather than the aspirational schema.

### `@@subscribe`: SSE subscriptions for RPC transport (#183, #390)

A spike into whether the existing SSE streaming machinery could cover one-way `@@subscribe` model-event feeds — previously locked to a still-unimplemented WebSocket-only design — concluded yes: the cancellation objection that ruled out SSE for arbitrary streaming doesn't hold for a fire-and-forget, no-replay, one-subscription-per-connection feed (#183). `@@subscribe` — a bare model attribute requiring `@@emit(...)` and `transport rpc` — now emits `OpKind::Subscription`, dispatched at `GET /rpc/subscribe/{op_id}` through the existing outbox-drain pipeline. Backpressure is a bounded per-subscription channel that closes on overflow, surfaced as a terminal SSE error event (#390).

### gRPC: procedures and server-streaming (#208)

`procedure` declarations now reach the tonic gRPC service: unary procedures get a `UnaryService` method and list-arity procedures get a `ServerStreamingService` method, both dispatched through the same handler function — and therefore the same policy/audit pipeline — that REST and RPC already call.

### Correctness fixes: JSON columns, keyword-named fields, and route derivation

Both database backends persisted `Json`-typed columns through `cratestack_core::Value`'s own externally-tagged `Serialize`/`Deserialize` instead of plain JSON, so an empty map landed on disk as `{"Map": {}}` — breaking any read of jsonb the framework didn't write itself, and native `jsonb`/`->`/`->>` queries. Fixed on Postgres via a new `cratestack_sqlx::Json<T>` newtype (#162), then equivalently on the embedded rusqlite backend (#395).

A field named after a Rust keyword (`match`, `type`, `ref`, `move`, ...) emitted uncompilable code in every generated struct, decode impl, and client — fixed by funneling Rust-identifier emission through a shared `ident()` helper that emits raw-identifier form where one exists, and rejecting `self`/`Self`/`super`/`crate` at schema-parse time (#398).

The server's real Axum route derivation and the TypeScript/Dart client generators' route derivation were three independently-maintained algorithms that agreed on plain PascalCase names but diverged on any name containing a literal underscore, producing client routes the server never registered — unified onto a single `cratestack-core::route_naming` module (#345). Separately, the TypeScript `swr` preset's per-model file name could collide for two distinct, parser-valid model names, silently clobbering one file's output with the other's; generation now rejects that up front (#344).

### Parser and policy correctness

Field names were deduplicated on the raw `.cstack` name rather than `to_snake_case`, so two fields normalizing to the same SQL column compiled to valid Rust but emitted a table with a duplicate column and no error; and reserved-identifier rejection only ran at field call sites, so a colliding enum name failed later as an opaque parser error at the macro invocation. Both are now checked at every identifier site (#408).

`@allow(true)`/`@deny(true)` — a bare boolean literal as a procedure-level policy clause — failed to parse, falling through to field resolution and erroring as an unknown input field; a new `ProcedurePredicate::Literal(bool)` variant gives schema authors a direct way to mark a procedure public (#405, #406). Separately, `push_action_policy_query` wrapped its emitted SQL in parentheses only on its `@@deny`-present branch, leaving the other branch's boolean grouping dependent on the caller; both branches now wrap unconditionally. Fixing this also revived the `policy_db*` integration test suite, which had sat entirely `#[ignore]`d and run nowhere in CI (#410).

### Small fixes, CI, and release plumbing

`cratestack-cli` gains a working `--version`/`-V` flag (#201). `cargo deny check` is now a real gate — CI previously caught its non-zero exit, logged it as expected, and continued — with every existing license/advisory hit resolved on its merits (#409). `just bump` previously replaced every occurrence of the bare version literal across every `Cargo.toml`, which also rewrote unrelated third-party dependencies pinned to the same version number; the 0.7.1 → 0.7.2 bump itself broke this way, turning `serde_urlencoded = "0.7.1"` into a nonexistent `"0.7.2"`, and the replace is now scoped to actual `version =` keys (#432).

## 0.7.1 (2026-08-03)

A follow-up fix to 0.7.0's `FindMany<Model>`: `include_client_schema!` never generated the `PostFindManyInput`-style types the server composer did, so any schema using `FindMany<Model>` as a procedure argument failed Rust HTTP client generation with "cannot find type." Fixed by splitting the shared type generation out of the server-only query-builder wrapper, with a new regression test proving the wire format round-trips through the client-generated types, not just that the macro compiles (#381).

## 0.7.0 (2026-08-03)

### `FindMany<Model>`: built-in search-with-filters procedure argument (#371)

Procedures gain a built-in generic argument type for search-with-filters, following up on `PageInput` (0.6.7). A procedure can now declare `searchPosts(query: FindMany<Post>, page: PageInput): Page<Post>` — filtering/sorting and pagination stay two independent, orthogonal arguments. It's restricted to procedure-argument position, and `Model` must be a declared model rather than a `type` block, since filtering needs a real table's columns to validate field names against.

The shape went through a real redesign mid-implementation: the first cut reused the existing `list` route's flat string-DSL (`{ where: String?, orderBy: String? }`). That was replaced before release with structured, per-model typed filters across all three generators — Rust server (`PostWhere`/`PostSortField`/`PostFindManyInput`, built on a shared `FieldFilterInput<V>`), TypeScript, and Dart (default/riverpod) — since a caller-facing query language is worth getting typed once rather than passing through a string. `orderBy` is a `Vec<OrderByClause>` rather than a single object, since neither `serde_json::Map` nor JS object key order is guaranteed to preserve multi-key sort order.

Server-side codegen adds one `build_<model>_query_from_find_many` function per model, reusing the model's own already-generated list-route filter/sort machinery, so a `FindMany<Post>` argument validates against exactly the same allowed fields a REST `?where=` on `/posts` already does. The client-side `FindMany` type is deliberately non-generic across Rust/TypeScript/Dart, since the wire shape never depends on the model. Two real bugs surfaced only by running generated output through real tooling: `SearchPostsArgs` decoding via a now-nonexistent bare `FindMany.fromWire`, and a generated `models/post.dart` missing its `shared_types.dart` import.

Also, `cratestack-sqlite`'s README now documents the `codec-json` feature, which had gone undocumented since 0.6.8.

## 0.6.8 (2026-08-03)

Release pipeline and dependency-maintenance patch, no framework or generated-code changes. `release-cli.yml`'s five `publish-npm-*` jobs pinned `npm@^11` instead of always installing latest, after npm 12 changed `npm pack --dry-run --json`'s output shape and broke `@napi-rs/cli`'s pack detection (#369); `prepare-release.yml`'s Node version was bumped from 20 to 24 to match `ci.yml`, after an `undici`/Node version mismatch broke `swr_hooks_invalidation`'s vitest run on that job specifically (#377).

TypeScript, vitest, biome (1→2), turbo, and the vscode extension's dependencies move forward across the pnpm workspace, along with client codegen templates and example projects — each bump verified with a real build/typecheck/test run; two pins were deliberately held back after checking against the real toolchain (Dart `riverpod`'s analyzer ceiling, `embedded-browser-webpack`'s TypeScript pin against `ts-loader` 9.6.2). Also fixes #358: the `riverpod` preset's generated `build_runner` cap was `<2.15.0`, but the actual break is in 2.15.2; the cap is now `<2.15.2` (#364). A prior wasm32 import fix to `embedded-browser-vite`'s `mod wasm` block had never been copied to three sibling example crates carrying the identical block, so all three were silently failing to build for wasm32 (#373). Five further commits bring READMEs, example indexes, and CLI docs back in sync with shipped code, following an audit that found version pins as stale as 0.2.2 (#372–#376).

## 0.6.7 (2026-08-03)

### Embedded backend gets real pagination; new built-in `PageInput` (#363, #366)

`@@paged` shapes a generated `list` route's response envelope on REST/RPC/gRPC, but `include_embedded_schema!` generates no routes at all, so a `@@paged` model there previously just compiled to nothing, silently. The first fix attempt rejected `@@paged` on embedded schemas outright with a `compile_error!`, mirroring the existing `@@materialized`-on-embedded guard — but per the maintainer's pushback on that approach ("this is our software, what's blocking us?", #366), rejection papered over a gap that `cratestack-rusqlite` already had the pieces to close.

`FindMany` (on both models and views) now has `.paginate(PageInput) -> Page<M>` and `.paginate_in_tx`, backed by a new `render_count` and a real `COUNT(*)` run inside the same connection borrow as the paginated `SELECT`, so the count and the page it describes can't be split by a concurrent write. It's available unconditionally on every model, the same "no attribute wiring needed" treatment `@@audit`/`@@emit` already get.

Alongside this, a built-in `PageInput` procedure-argument type (`{ limit: Int?, offset: Int? }`) fills a gap on the request side that `Page<T>`/`PageInfo` already covered on the response side. `PageInput::resolve(max_limit)` applies the same `MAX_LIST_LIMIT` clamp rule generated `list` routes already use, and is wired through the Rust server and the Rust/TypeScript/Dart clients. gRPC's existing `@@paged`-independent behavior was confirmed already correct and left unchanged.

### Release and CI plumbing

Three small fixes: the `publish-npm-cbor-node` release job failed `tsc` on its first real OIDC publish attempt because `napi artifacts` only copies `.node` binaries, not `native.mjs`/`native.d.mts`, and the job never built `@cratestack/ts-types` as its own step (#362). The same PR also fixed `prepare-release.yml`'s `git add` list, the root cause of 0.6.6's bump PR landing with the lockfile bumped but the cbor family's `package.json`s left stale.

A new `install-cratestack-cli` composite GitHub Action downloads a prebuilt `cratestack-cli` binary for the runner's OS/arch, verifies its SHA-256, and adds it to `PATH` with no Rust toolchain required (#365). Getting it working against the real 0.6.6 release surfaced two platform-specific bugs in the same PR: a `grep -m1` piped from `curl` under `set -o pipefail` could SIGPIPE-abort the script, fixed by buffering and parsing with `jq`; and Windows' Git Bash `tar` can't read `.zip` archives, fixed by branching to PowerShell's `Expand-Archive` on Windows.

Finally, `examples/react-vite-daisyui`'s `tsconfig.json` was missing `allowImportingTsExtensions`, so `npm run typecheck` failed with TS5097 on its own `.ts`/`.tsx`-suffixed sibling imports (#367).

## 0.6.6 (2026-08-03)

Release-and-CI plumbing only, hardening the `@cratestack/cbor-node` npm publish pipeline: three fixes land back-to-back, working through the still-unproven release path one failure at a time. The Windows leg of the 0.6.5 release failed because the napi build step's multi-line `run:` used `\` line continuations, fine under bash but parsed by PowerShell as a unary `--` operator; `shell: bash` is now pinned on that step (#356). That surfaced a chain of pipeline gaps that had never been exercised end-to-end — `build-cbor-node`'s job gate excluded `workflow_dispatch`, `publish-npm-cbor-node` was missing the `napi create-npm-dirs`/`napi artifacts` scaffolding steps its own `prepublishOnly` hook depends on, and its artifact download switched from a flat layout to per-platform subdirectories to match how `napi artifacts` matches `.node` files to targets. Separately, `cratestack-cbor`/`-cbor-node`/`-cbor-web`'s `package.json` versions had been stuck at 0.5.2, which — because pnpm's `link-workspace-packages=true` only symlinks a workspace dependency when the pinned semver matches — silently resolved their `@cratestack/ts-types` dependency to the real published 0.5.2 package instead of local workspace source, so all three packages had quietly been building against three-versions-stale types (#357).

The same lockstep-version gap then broke the 0.6.6 bump itself: `prepare-release.yml`'s bump-PR `git add` list still only staged the original 11 api-family `package.json` files, so `just bump` wrote the cbor version bump to disk but git never committed it, breaking every JS CI job with `ERR_PNPM_OUTDATED_LOCKFILE`. Fixed by adding the cbor family to this workflow's own `git add` list (#360). No framework or generated-code behavior changed in this release.

## 0.6.5 (2026-08-03)

### `cratestack-api`: a third facade, and `db = None` genuinely drops `sqlx` (epic #326 done)

Epic #326's last story lands: `cratestack-pg` gains a default-on `postgres` Cargo feature gating `sqlx`/`cratestack-sqlx`, so a `db = None`-only consumer can `default-features = false` and have `sqlx` genuinely absent from `cargo tree`, not just unused. `rpc-procedures`, `rpc-batch`, `rpc-streaming`, and `rpc-batch-debounce` move off their old `connect_lazy(&url)` workaround onto real `datasource { provider = "none" }` + `db = None` schemas (#329).

A direct follow-up (#347, landed as #350) goes further: `crates/cratestack-api` is a new, fully separate third facade — following `cratestack-pg`'s and `cratestack-sqlite`'s exact structural pattern — that never depends on `cratestack-sqlx` under any feature. A new compile-time guard, `guard_server_postgres_backend`, turns `db = Postgres` under this sqlx-less facade into one clear `compile_error!` instead of a wall of unrelated resolution errors. `examples/no-database-verification-api` proves the absence with a real `cargo tree` check, and all four `db = None` examples migrate onto the new crate; `cratestack-pg` + `default-features = false` keeps working as the pre-existing alternative path.

### Native Rust gRPC client for `transport grpc` schemas (#209)

`include_client_schema!` now generates a typed, tonic-based Rust client for `transport grpc` schemas — one method per model CRUD verb, matching the surface the server runtime and the gRPC-Web TypeScript client already expose. The compile-time guard that unconditionally rejected client-side gRPC codegen splits into `guard_client_grpc_transport` (now feature-gated) and `guard_embedded_grpc_transport` (still an unconditional reject). `cratestack-client-rust` gains an optional `grpc` feature and a `CratestackGrpcClient<T>` runtime with its own `RequestAuthorizer`/schema-sha handling and a deliberate, documented reimplementation of `cratestack-grpc`'s canonicalization, kept byte-identical to the server side without pulling axum/tonic-web into client-only binaries.

### TypeScript client: query builders and Node ESM correctness

RPC transport's generated list/`use` hooks previously took only an untyped `Record<string, unknown>`. A new `CratestackRpcListQuery`/`toRpcListInput` pair — the RPC counterpart of REST's existing query-builder pair — gives it the same typed shape (#333, landed as #352).

More seriously: `CratestackFetchQuery` typed `where`/`filters`/`orFilters` as JSON-ish objects, and its fallback `JSON.stringify()`'d them into the URL — but the real server grammar (`FilterExpressionParser` in `cratestack-axum`) is a flat-text DSL, not JSON, so any caller populating these fields as documented got a hard 400 from a real server, with zero test coverage catching it. Fixed to mirror the Dart client's convention: `where`/`or` are now pre-built DSL strings, and `filters` is a flat `Record<string, string>`. **This is a breaking change to `CratestackFetchQuery`'s public shape**, fixed directly per this repo's hard-cutover convention (#351). A new test runs the generated client under real Node and feeds the captured request URL through the real `cratestack-axum` parser.

A third fix: every relative import/export in the TypeScript templates was extensionless, which resolves under a bundler but fails under plain Node's native ESM resolver with `ERR_MODULE_NOT_FOUND` — fixed at the template level and in the `swr` preset's dynamically-assembled cross-file imports (#315, landed as #343).

### Dart client: riverpod query forwarding and analyzer cleanliness

REST's generated list/get `@riverpod` providers now accept optional query objects and forward them to the underlying API calls, instead of always calling with zero arguments. This required hand-rolled `operator ==`/`hashCode` on those query classes — Riverpod's family providers dedupe by argument *value* equality, and a freshly-constructed-but-equal query previously never hit the cache. RPC's list provider forwards an untyped `IMap<String, Object?>` bag rather than a typed query builder, a documented decision to expose the existing untyped RPC contract now rather than design a full typed one (#331, landed as #349). Two pre-existing `flutter analyze` info-level findings are also fixed across every generated package, bringing default-severity `flutter analyze` to zero issues everywhere (#308, landed as #346).

### FIPS crypto: false success made impossible

`install_fips_crypto_provider()` returned `Ok(())` without installing any crypto provider, and the `aws-lc-rs` feature it gates enabled nothing — a false assurance in a compliance-facing API. Wiring a real provider needs the TLS backend to become a genuine per-crate choice first, which is out of scope here; until that lands, enabling the feature is now a hard `compile_error!` instead of a silent no-op (#334, landed as #341).

### Plumbing

The `@cratestack/cbor`/`cbor-node`/`cbor-web` family (shipped earlier but never released to npm) gets its release wiring: four new jobs in `release-cli.yml` build and publish the napi-rs native addon, the wasm-bindgen browser build, and the pure-TS umbrella in dependency order (#342). The gRPC e2e test added alongside the new Rust client was breaking every default `cargo test --workspace` run because nothing started its target server first; it now skips quietly on connection failure (#353). CHANGELOG.md also gained its 0.5.0–0.6.4 backfill in this range (#339).

## 0.6.4 (2026-08-02)

### Dart Riverpod preset: build_runner integration + example app (epic #297 done)

`generate-dart` gains an opt-in `--run-build-runner` flag that shells out to
`dart run build_runner build --delete-conflicting-outputs` after
generation, with a clear "no Dart SDK found on `PATH`" error (naming the
manual fallback command) rather than a panic or a silent no-op when the
tool isn't there. A real Flutter app, `examples/flutter-riverpod`, consumes
a `--preset riverpod` client with zero hand-written providers — the
epic's own success metric — overriding the adapter provider to point at a
real local server. This is the fourth and final story of epic #297,
closing it (#303).

### `datasource none`: procedures-only servers without a database (epic #326, in progress)

`.cstack` schemas can now declare `datasource { provider = "none" }`,
rejecting any `model` block in the same schema, and `include_server_schema!`
cross-checks this against its own `db` argument for the first time — until
now `db = Postgres` was the macro's only accepted value and the argument
was silently discarded rather than checked against anything. `db = None`
then generates a genuinely `PgPool`-free `Cratestack`/router — not an
unused parameter or an always-`None` `Option<PgPool>`, a structurally
different generated type, with `ModelRouterState` and the event module
omitted entirely rather than compiled in as dead code. A real integration
test round-trips a procedure call over HTTP with zero `sqlx` import
anywhere in its own setup (#327, #335). `sqlx` Cargo-feature-gating and
migrating the framework's own examples off their `connect_lazy` workaround
are tracked separately and still open (#329).

### Dart Riverpod preset: real equality for generated data classes

Every `riverpod`-preset generated Dart data class (models, `Create<M>Input`/
`Update<M>Input`, procedure argument wrapper types) now gets real
`operator ==`/`hashCode`/`copyWith` via `dart_mappable`. Without this, a
`@riverpod` "family" provider taking a generated class as its argument
never settled — Riverpod dedupes family providers by argument *value*
equality, and a freshly-constructed instance on every rebuild (an entirely
ordinary pattern) never matched a prior instance by identity, so the
provider restarted `AsyncLoading` forever. Reproduced live against a real
server before the fix. Relation-list fields also switched to
`fast_immutable_collections`' `IList<T>` in place of `List<T>` (#325, #336).

## 0.6.3 (2026-08-02)

Small follow-ups to the two client-preset epics landed in 0.6.1:

* One `@riverpod` provider per operation for the Dart `riverpod` preset —
  parameterized `Future` providers for reads, `AsyncNotifier` controllers
  for writes — built by watching the preset's existing per-model DI layer
  rather than reconstructing adapter access from scratch (#302).
* TypeScript `swr` preset: a `@@paged` model's generated file was missing
  its `Page`/`PageInfo` import, a real `tsc` failure (#318).
* TypeScript REST client: widened the `SCHEMA_SHA256` constant's type to
  `string` — with a real, non-empty schema hash baked in, TypeScript
  inferred a literal type and flagged the runtime's own `=== ""` check as
  having no possible overlap (#323).

## 0.6.1 (2026-08-02)

### TypeScript SWR preset

A second, opinionated TypeScript client preset, `--preset swr`: one file
per model with plain, framework-free async functions underneath and a
`useSWR`/`useSWRMutation` hook per operation on top, so the functions stay
usable from a script, a server action, or a test with zero React/SWR in
the import graph. Cache invalidation follows an explicit, documented rule
(create invalidates the list; update invalidates the list and the detail;
delete invalidates the list and drops the detail) proven by a real test
asserting exact refetch counts, not just that the code compiles. A real
end-to-end example app, `examples/react-vite-swr`, demonstrates it against
a live server with browser-observed cascading invalidation (#304, #305,
#320). Also fixes a real duplicate-key collision in the *default*
react-query preset's key object, surfaced while building the example
(#319).

### Dart Riverpod preset (started)

First story of a new, parallel Dart preset: `--preset riverpod` fans
generated output out to one file per model instead of a single monolithic
`models.dart`/`apis.dart` (#301). Also closes a real, pre-existing gap —
nothing in CI had ever run generated Dart through an actual Dart/Flutter
toolchain before this; the existing snapshot tests only assert on
generated *text*, which can't catch a missing import or an undefined
symbol (#300).

## 0.6.0 (2026-08-02)

### `@cratestack/api` split into a 9-package family

`@cratestack/api` is split into `ts-types`, `link-batch`, `link-logger`,
`runtime-fetch`, `runtime-axios`, `validator-zod`, `validator-yup`,
`adapter-tanstack-query`, and `adapter-rtk`, so a client that only needs
types isn't forced to ship batching/logging/HTTP-adapter code it never
calls. `@cratestack/api` itself becomes a backward-compatible re-export
shim over the new packages (#265). A follow-up fixes `link-batch`
silently dropping per-call headers/fetch overrides/codec choice when
partitioning a batch — flushes are now grouped by transport signature
instead of merged blindly (#273).

### RPC streaming: genuine incremental delivery + a first-party CBOR codec family

`@stream`-marked procedures now stream for real: the server encodes and
flushes each item onto the HTTP body as it's produced instead of
buffering the whole sequence before the first byte goes out, with a
CBOR-tagged sentinel (tag 48900) as the final item on a mid-stream
failure (#292, #294). The original design's mid-stream error mechanism (a
trailing content-type chunk) turned out to be physically unrealizable
over HTTP/browser `fetch` and was corrected before implementation, not
after (#289). The generated TypeScript RPC client gets a matching
`RpcStreamLink` chain and a hand-rolled CBOR-seq boundary scanner, tested
against real wire bytes captured from the generated server rather than
hand-built fixtures (#277, #299).

Alongside this, a new first-party CBOR codec family —
`@cratestack/cbor-node` (napi-rs), `@cratestack/cbor-web` (wasm-bindgen),
and `@cratestack/cbor` (an umbrella package with conditional `node`/
`browser`/default exports) — wraps the existing Rust `cratestack-codec-cbor`
crate for both native Node and browser targets (#286, #287, #288, #291,
#293).

### Migrations: foreign keys, `onDelete`/`onUpdate`, unique indexes

`@relation` fields now emit real `FOREIGN KEY` constraints (#260, #261),
with `onDelete`/`onUpdate` actions declarable in the schema (#268), and
model-level `@@unique([...])` now emits a real `CREATE UNIQUE INDEX`
(#266).

### Other fixes

* Two `sqlx` fixes preserve SQLSTATE/constraint-name classification on
  generated write queries and batch write queries, instead of collapsing
  every constraint violation into a generic error.
* Migration/DDL SQL is no longer split on literal `;` characters, which
  broke on any statement containing one inside a string or comment.
* Two macro-side fixes replace an exponential REST `orderBy` match-arm
  enumeration with runtime relation-hop resolution (#279), and drop a
  vestigial `Result` that was masking a real SQL-correctness gap (#280).

## 0.5.2 (2026-08-02)

Infra only: `npm publish` switched from long-lived tokens to OIDC trusted
publishing — no user-facing change (#221).

## 0.5.1 (2026-08-01)

Test-only: adds a relation-connectivity regression fixture closing a gap
left by the exponential-relation-codegen fix in 0.5.0 — no user-facing
change (#257).

## 0.5.0 (2026-08-01)

**Breaking:** relation codegen for models with many interrelated `@relation`
fields was exponential in the number of relations, making some real-world
schemas fail to compile at all. Fixed to be linear (#253). Also drops
stale version pins from example crates' path dependencies (#254).

## 0.4.18 (2026-07-31)

### Studio: Postgres row-keying fix, persistent audit log, EXPLAIN

`Row` is documented as keyed by `.cstack` field name, and the UI, cursor
pagination, relation-follow, and audit log all rely on that contract — but
the Postgres data source keyed rows by raw snake_case column name instead.
camelCase and snake_case coincide for single-word fields, which is why this
went unnoticed; on a realistic schema, every multi-word field silently broke
table rendering, pagination's "Next" button, relation follow, and the audit
log's recorded PK. Fixed by aliasing each projected column to its field name.

Also new: an opt-in persistent audit log (`[workspace] audit_file`, an
append-only JSONL sidecar replayed on boot, replacing in-memory-only
history), and query plans (`GET .../sql?explain=true` plus an "Explain"
toggle in the Studio UI). (#240)

### Studio: edit form no longer corrupts NULL columns on save

Opening a row with a NULL nullable column, clicking Edit, and clicking Save
without changing anything wrote the literal string `"—"` (the read-only
table's display placeholder for NULL) into that column instead of leaving it
NULL. The edit-form snapshot was reusing the display-formatting helper to
seed the editable form; it now maps NULL to the same "no value" sentinel
every editor widget already uses, matching what the save path already
expects. (#242)

## 0.4.17 (2026-07-30)

### Parser and migrate hardening around storage-type edge cases

A cluster of related fixes tightening what the parser accepts and what
`cratestack-migrate` emits, found while generating a round-trip test for
every builtin scalar/enum across Postgres, SQLite, and the LSP (#232, #237):

* Postgres now stores enums as `TEXT` + `CHECK` (not a native `CREATE TYPE
  ... AS ENUM`), and bareword enum defaults are quoted correctly in the
  emitted DDL (#233).
* `type` blocks can no longer be used as a model field's storage type —
  they're a payload shape for procedures, not a column type (#235).
* List-arity scalar/enum model fields are rejected on datasource-backed
  schemas, since there's no portable column type for "array of enum" across
  both backends (#229, via #236).
* Reconciled `#233`'s enum-list emitter test with `#229`/`#236`'s new
  list-arity parser rejection — the two landed close together and briefly
  disagreed on enum-list fields (#238).
* `Json` now derives `Default`, fixing a compile failure under
  `include_embedded_schema!` for models with a default-valued `Json` field
  (#234).

### Other fixes

* Rate-limit store errors are logged instead of failing the request
  silently (#215).
* A CI-only quality pipeline (informal replacement for a paid SonarQube
  instance) landed across several follow-up PRs — pinned-action scanners,
  PR review-comment output instead of Check annotations, and a documented
  gap-until-landed note for interim coverage (#216, #218, #220, #222, #225).

### Dart: native gRPC client generator

`generate-dart` gains a native gRPC client generator for schemas declaring
`transport grpc`, plus channel-shutdown and per-call option exposure on the
generated client, and gRPC-specific example/test templates (a pre-existing
RPC-transport example/test bug was caught and fixed during review) (#210,
via #211, #213, #214).

## 0.4.16 (2026-07-26)

No code changes. A clean recut of the release pipeline after v0.4.14 (which
shipped GitHub-Release-only by deliberate choice) and v0.4.15 (crates.io +
GitHub Release succeeded, but both npm publishes failed with `EOTP` — the
configured `NPM_TOKEN` wasn't an Automation-type token). v0.4.16 is the
first release to publish successfully to crates.io, npm (`@cratestack/cli`
and `@cratestack/api`), and GitHub Release binaries in one shot, with zero
manual publish steps.

## 0.4.15 (2026-07-26)

`cut-release-tag.yml`'s tag push now uses a dedicated `RELEASE_PAT` instead
of the default `GITHUB_TOKEN` (#197). GitHub's anti-recursion protection
silently no-ops any downstream workflow trigger from a push made with the
default token — the tag itself lands fine, but `release-cli.yml` never
fires off it. A PAT-authored push is treated as a normal external push and
correctly cascades into the rest of the pipeline.

## 0.4.14 (2026-07-26)

### Protobuf + gRPC support

`.cstack` schemas can now declare `transport grpc`, generating `.proto`
message/enum definitions (with a field-number lockfile so wire numbers
don't silently renumber across schema edits) and gRPC service surfaces.
Design doc (#166) and implementation (across #168–#172) landed same-day
(#167, #176). CRUD-only for this release — procedure/streaming support and
a Rust gRPC client were carved out as follow-up tickets.

### Schema-fingerprint drift header

Every response now carries an `x-cratestack-schema-sha` header — a
warn-only fingerprint of the server's schema, so a client running against
a stale generated SDK can detect drift without a hard version pin. Shipped
for the Rust server first (#179), then Dart and TypeScript REST/RPC clients
(#180).

### RPC client DX: composable link chain

The generated TypeScript RPC client gains a composable `RpcLink` chain
(request/response middleware — logging, batching, auth injection, etc.),
published alongside a new `@cratestack/api` npm package carrying the
batching link and other cross-cutting concerns out of the generated code
itself (#182, #186).

### CI-driven release pipeline

The first version of the fully automated release flow: a `prepare-release`
workflow bumps versions and opens a PR, merging it auto-tags via
`cut-release-tag.yml`, and the tag push triggers `release-cli.yml` to
publish crates.io + npm + GitHub Release binaries with no manual steps
(#188). Landed rough — this version alone needed eight follow-up fixes to
get a real dry run and then a real dispatch through the pipeline end to
end: missing GTK/WebKit deps in CI (#189), the release-check test stage
needing a bundled Studio UI first (#190), `cargo publish --dry-run`
needing `--allow-dirty` (#191) and `--no-verify` (#192) in dry mode, dry
mode needing to skip non-leaf crates entirely since a never-published
version can't resolve as a dependency (#193), and two npm `pnpm install`
call sites needing to skip the `cratestack-cli` binary download since
neither actually needs it (#194, #196). (The pipeline's tag-push
anti-recursion bug that blocked this version's own crates.io/npm publish
is the separate v0.4.15 fix above.)

### Other fixes

* `Cuid` scalar validation relaxed to accept `cuid2` ids, not just the
  original `cuid` format (#150, via #158).
* `cratestack-redis` gains a `tls-rustls` feature for `rediss://`
  connections (#151, via #159), and later in this same version switches
  to caching and reusing a single connection instead of opening one per
  call (#175, decision recorded in #177).
* Design doc proposing an `Extensions` concept, reframing the rate-limiting
  half of #139's declarative-surface decision (#160).
* Clippy `too_many_arguments`/`type_complexity` cleanup in `cratestack-sql`
  and `cratestack-sqlx` (#184, #185).

## 0.4.13 (2026-07-22)

A dense release — nine PRs, several the direct result of a full backlog
pass over long-open tickets:

* **`--check` drift-detection mode** for `generate-typescript` /
  `generate-dart`: exits non-zero if generated output would differ from
  what's on disk, for CI gates (#141).
* **Prebuilt `cratestack-cli` binaries** — GitHub Releases, `cargo-binstall`
  support, and an npm-installable wrapper, so installing the CLI no longer
  requires a Rust toolchain (#142).
* **`--full-selection` flag** for `generate-typescript`, emitting a fully-
  required model type alongside the normal partial-selection type (#140).
* **`cratestack diff`** — a new CLI subcommand that diffs two `.cstack`
  schemas and classifies each change by its effect on the generated wire
  contract (breaking / additive / internal-only), exiting non-zero on any
  breaking change so it can gate CI on schema PRs (#144).
* **Migrate baselining design spike** — a doc-only PR spiking Postgres
  live-schema introspection for baselining an existing database against a
  `.cstack` schema, not yet implemented (#135, via #143).
* **Composite primary keys** via `@@id([...])` — parser and
  `cratestack-migrate` DDL support landed; query builders, clients, and
  policy integration are follow-up work (#145).
* **Idempotency/rate-limiting declarative-surface decision** — a design
  doc settling that rate-limiting stays an imperative, hand-wired concern
  permanently, while idempotency is deferred pending an `OpExecutor` gate
  (#139, via #146).
* **`dbgenerated()` fix** — emits valid SQL instead of a broken default
  expression, and warns when the expression can't be verified against the
  target dialect (#148).
* **Type-block field-reference fix** — qualifies a `type` block's
  references to model types correctly instead of emitting an ambiguous
  reference (#137, via #147).

## 0.4.12 (2026-07-22)

The generated TypeScript RPC client runtime now satisfies its own
`exactOptionalPropertyTypes` compiler setting — a previous release enabled
the stricter TS option in the generated code but the runtime itself wasn't
compliant, so consumers with the same setting on saw type errors (#129).

## 0.4.11 (2026-07-22)

* Fixed `Page<T>`/`PageInfo`'s generated TypeScript shape not matching
  what the wire actually sends (#124).
* Capped the `list` route's page-size limit consistently across REST and
  RPC transports, and made the RPC codec pluggable rather than hardcoded
  (#126, closing #123 and #125).

## 0.4.10 (2026-07-22)

A round of audit-driven correctness fixes: a self-deadlock in the audit
path, a wrong soft-delete snapshot, a server-only field leaking into the
generated TypeScript client, and incorrect gating on TypeScript's generated
`create` calls (#120) — plus a fix for cross-binary test table-name
collisions inside `cratestack-pg`'s own test suite (#121).

## 0.4.9 (2026-06-17)

* Dart's CBOR decoder now normalizes decoded maps to `Map<String,
  Object?>` instead of a more loosely-typed map shape (#115).
* Fixed the `sqlite_offline_first` example failing to compile standalone,
  and guarded the embedded examples in CI (#106).

## 0.4.8 (2026-06-15)

Studio UI chrome revamp: reworked visual chrome and a multi-`.cstack`
target switcher, so one running Studio instance can browse several
schemas' targets from the same UI (#105). The repo also adopted an
AI-governance kit for issue/PR templates and contribution process around
this time (#104).

## 0.4.7 (2026-06-08)

For schemas using `transport rpc`, the op id is now the canonical request
identity — the value request signing and tracing key off, rather than an
incidental routing detail (#102).

## 0.4.6 (2026-06-07)

Fixed `BatchableCall` mis-encoding `None` optionals as a CBOR empty array
instead of a CBOR null in the Rust client (#100).

## 0.4.4 (2026-05-20)

* Published a documentation-only `cratestack` landing crate to crates.io
  — after the umbrella-facade split below removed the real `cratestack`
  crate, this keeps the name from going orphaned/squattable and points
  visitors at `cratestack-pg` / `cratestack-sqlite` (#97, doctests
  disabled on it in a same-day follow-up, #98).
* `CoolError` now preserves the full typed `DatabaseError` chain instead
  of flattening it, so callers can match on the underlying driver error
  (#99).

## 0.4.3 (2026-05-19)

Follow-up to the facade split below: fixed generator-fixture test paths
that still pointed at the removed `cratestack` umbrella instead of
`cratestack-pg` (#96).

## 0.4.2 (2026-05-19)

### Breaking: the `cratestack` umbrella facade was split

The single `cratestack` umbrella crate is gone. It has been carved into
two strictly disjoint sub-facades that consumers pick between via
Cargo's `package =` rename:

```toml
# Backend service (Postgres + Axum + generated Rust client runtime)
cratestack = { package = "cratestack-pg", version = "0.4" }

# Embedded / mobile / desktop / wasm (rusqlite + shared surface)
cratestack = { package = "cratestack-sqlite", version = "0.4" }
```

Schema macros (`include_server_schema!`, `include_embedded_schema!`,
`include_client_schema!`) continue to emit `::cratestack::*` paths
unchanged. Strict disjointness is enforced by what the consumer picks,
not by the macro.

**Why this matters in practice:**

* `cratestack-pg` does not pull in `cratestack-rusqlite`, so
  `libsqlite3-sys` is no longer in the dep graph. Backend services can
  now depend on the official `sqlx` umbrella crate (which optionally
  declares `sqlx-sqlite`) without tripping Cargo's `links = "sqlite3"`
  collision rule. Downstream `sqlx-shim` workarounds can be deleted.
* `cratestack-sqlite` keeps compiling on `wasm32-unknown-unknown`; it
  also exposes `cratestack-client-rust` on native targets so hybrid
  consumers (e.g. a Tauri or NAPI shell that ships an embedded DB
  *and* calls a remote backend) can still use `include_client_schema!`
  alongside `include_embedded_schema!`.

### Breaking: `Projection` trait moved + renamed

The `Projection` trait — implemented by every model's macro-emitted
`Selection` type to decode projected query responses — has moved from
`cratestack-client-rust` into `cratestack-core` and been renamed
**`ProjectionDecoder`**. The previous name collided with the SQL value
type `cratestack_sql::Projection<T>` (the actual `.select()` result
wrapper), which was the more central, user-facing meaning of the name.

* Old: `cratestack::client_rust::Projection`
* New: `cratestack::ProjectionDecoder`

`cratestack-client-rust` keeps re-exporting the trait under both
`ProjectionDecoder` and the deprecated `Projection` alias for one
release. Macro-emitted code now references the new name, so most
codebases will see no source-level impact.

### New: SQL views (ADR-0003)

A new `view` block in `.cstack` declares a read-only, SQL-defined
projection over one or more existing `model` blocks. Views generate
a typed Rust struct, a read-only delegate, and `CREATE VIEW` DDL
during migration generation, with the same `@@allow` policy
enforcement models get.

```cstack
view ActiveCustomer from Customer, Order {
  id          Int       @id  @from(Customer.id)
  email       String         @from(Customer.email)
  orderCount  Int

  @@server_sql("""
    SELECT c.id, c.email, COUNT(o.id)::int AS order_count
    FROM   customers c
    LEFT JOIN orders o ON o.customer_id = c.id
    GROUP  BY c.id, c.email
  """)
  @@embedded_sql("""
    SELECT c.id, c.email, COUNT(o.id) AS order_count
    FROM   customers c
    LEFT JOIN orders o ON o.customer_id = c.id
    GROUP  BY c.id, c.email
  """)

  @@allow("read", auth() != null)
}
```

```rust
let cool = cratestack_schema::Cratestack::builder(pool).build();
let rows = cool.views().active_customer().find_many().run(&ctx).await?;
```

#### Capabilities

* **Both backends.** `@@server_sql` runs against Postgres; `@@embedded_sql`
  runs against SQLite. The `@@sql` shorthand applies to both with a
  cargo warning that portability is the developer's problem.
* **Materialized views (server only).** `@@materialized` emits
  `CREATE MATERIALIZED VIEW` + `CREATE UNIQUE INDEX <name>_pkey ON
  <name> (<id>)` and produces a `refresh()` method on the delegate
  that runs `REFRESH MATERIALIZED VIEW CONCURRENTLY`. Embedded
  builds with a `@@materialized` view hard-error at macro expansion
  time — SQLite has no materialized views.
* **Type-level read-only.** `ViewDescriptor` does not implement
  `WriteSource`, so the bound on `CreateRecord` / `UpdateRecord` /
  `DeleteRecord` / `UpsertModelInput` simply fails to hold — there
  is no runtime check, the type system refuses.
* **`@@no_unique` gets its own delegate.** Views declared
  `@@no_unique` return a separate `ViewDelegateNoUnique<V>` type
  that omits `find_unique` (and `refresh()`) at the type level, so
  a call like `runtime.views().<v>().find_unique(())` is a compile
  error rather than a runtime `WHERE  = $1` footgun.
* **Migration ordering is automatic.** `cratestack-migrate` lands
  `DROP VIEW` ops before column / table drops the view referenced
  and `CREATE VIEW` ops after the matching column / table adds, so
  body changes that overlap with column changes still apply
  correctly. Body changes are modelled as `Drop + Create` (not
  `CREATE OR REPLACE VIEW`) to preserve that ordering invariant.
* **Policy enforcement is the same machinery models use.**
  `@@allow("read", expr)` lowers into the same `ReadPolicy` array
  consumed by `push_scoped_conditions`. Only the `"read"` action
  is accepted; any other action is a parse error.

Landed end-to-end across eight PRs:
[#84](https://github.com/cratestack/cratestack/pull/84) (parser + IR +
validator),
[#85](https://github.com/cratestack/cratestack/pull/85) (`ReadSource`
/ `WriteSource` traits + `ViewDescriptor`),
[#86](https://github.com/cratestack/cratestack/pull/86) (polymorphic
read helpers),
[#87](https://github.com/cratestack/cratestack/pull/87) (generic
read builders + `ViewDelegate`),
[#88](https://github.com/cratestack/cratestack/pull/88) (macro
emission + `runtime.views()` accessor),
[#89](https://github.com/cratestack/cratestack/pull/89) (migrate IR +
diff + per-backend DDL),
[#90](https://github.com/cratestack/cratestack/pull/90) (policy
lowering),
[#91](https://github.com/cratestack/cratestack/pull/91) (integration
tests vs real Postgres + SQLite). ADR-0003 is `Accepted` in the docs
repo (`cratestack-docs` [#21](https://github.com/cratestack/cratestack-docs/pull/21)).

### Cleanup

* `cratestack-macros` no longer emits selection / projection helpers
  behind a `cfg(not(target_arch = "wasm32"))` gate — `ProjectionDecoder`
  now lives in `cratestack-core` and works on every target.
* The umbrella's banking / policy / migrations / isolation /
  validation / generated-client integration tests are now under
  `crates/cratestack-pg/tests/`; the SQLite e2e test under
  `crates/cratestack-sqlite/tests/`. No test logic was changed.

### Other fixes

* Projected-query decoding now tolerates a missing optional field instead
  of erroring, matching how a partial `SELECT` projection is actually
  expected to behave (#93).
* `codec-json` is now an opt-out feature on `cratestack-client-rust`
  rather than always-on (#94).
* CI's rustdoc build now points at `cratestack-pg`, the facade split's
  replacement for the removed `cratestack` umbrella (#95), and the
  release workflow gained a test-retry + `SKIP_TESTS` escape hatch for
  known-flaky suites (#81).

## 0.3.7 (2026-05-18)

No code changes beyond the version bump itself.

## 0.3.6 (2026-05-18)

Release tooling: publish order is now computed from `cargo metadata`'s
real dependency graph instead of a hand-maintained list, so a new crate
gets the right publish position automatically instead of needing a
manual list edit every time (#80).

## 0.3.5 (2026-05-18)

Release tooling: `release-publish` is now idempotent and resumable — a
partial failure partway through publishing the workspace can be re-run
and picks up where it left off instead of re-attempting crates that
already published successfully (#79).

## 0.3.4 (2026-05-17)

Studio's `eject` command is redesigned from a UI-fork-only tool into a
full-project starter scaffold: `cratestack studio eject --out <dir>`
now writes a runnable binary crate (`Cargo.toml`, `src/main.rs`,
`studio.toml`, an example schema) with the Leptos UI already bundled
in; `--with-ui` additionally unpacks the UI's Trunk sources for
front-end customization. The UI itself moves to a sibling
`crates/cratestack-studio/ui/` crate, embedded into the release binary
as a tarball rather than generated from templates, and
`cratestack-studio-generator` folds into `cratestack-studio` (#78).

## 0.3.3 (2026-05-17)

### Studio rewrite — Phase 1d + 4 (typed editors + power tools)

The final phases of the Studio rewrite. Phase 1d retires the
one-text-box-per-field approach in the create + edit forms; Phase 4
ships SQL preview, drift detection, CSV/JSON export, schema search,
an audit log, and constraint-aware error mapping.

**Typed editors (Phase 1d).** The create form and the drawer's edit
mode now dispatch on each field's declared scalar:

- `<select>` for enums (variants pulled from the schema)
- `<textarea>` for `Json` (free-form, parsed on submit)
- `<input type="datetime-local">` for `DateTime` (auto-normalized to
  `YYYY-MM-DDTHH:MM:SSZ` before the request)
- `<input type="number" step="any">` for `Float` / `Decimal`
- `<input type="number" step="1">` for `Int`
- `<select>` (true/false) for `Boolean`
- plain text for `String`, `Cuid`, `Uuid`, `Bytes`

The `/api/targets/:key/models` response gains `is_enum` and
`enum_variants` per field so the UI doesn't need a second round-trip
to populate the dropdown.

**SQL preview (Phase 4).**

```
GET /api/targets/:key/models/:model/sql?op=list|get|create|update|delete&pk=…
```

Returns the SQL Studio would run plus an ordered parameter list:

```json
{
  "driver": "postgres",
  "sql": "WITH inserted AS ( INSERT INTO \"posts\" …",
  "params": [ { "index": 1, "binding": "title", "kind": "text" }, … ]
}
```

API-backed targets return **501 UNSUPPORTED** — Studio doesn't render
SQL it doesn't run.

**Drift indicator (Phase 4).**

```
GET /api/targets/:key/drift
```

Compares declared columns (from the `.cstack` schema) against the live
database. Each model carries one of: `ok`, `drift` (column mismatch),
`missing_table` (table absent), `unsupported` (API-only target), or
`skipped` (no @id or unsupported PK type). The UI renders an amber
`⚠ drift` badge in the sidebar next to any model that doesn't match,
and a red `✕ table` badge for missing tables.

**CSV/JSON export (Phase 4).**

```
GET /api/targets/:key/models/:model/export?format=csv|json&limit=N
```

Streams up to `EXPORT_CAP = 10_000` rows through cursor pagination
under the hood and returns one body. Sets `Content-Disposition:
attachment; filename="<target>-<table>.<ext>"` so browsers download
the file. CSV uses RFC-4180-style escaping (quote-wrap on commas,
quotes, or newlines; double up embedded quotes).

**Schema search (Phase 4).**

```
GET /api/targets/:key/search?q=<term>
```

Case-insensitive substring over models, fields, enums (and variants),
types, mixins, procedures. Hits return `kind`, optional `model`,
`name`, and a short `detail` so the dropdown can present them. The
search bar in the header debounces on input and shows the dropdown
inline.

**Audit log (Phase 4).** Every successful write (CREATE / UPDATE /
DELETE) is appended to an in-memory ring buffer (cap **500**, FIFO
when full) attached to the workspace. The `Audit` button in the
header opens an overlay listing the most recent entries:

```
GET /api/audit?limit=N
```

Returns newest-first. Entries carry `id`, `at` (RFC-3339), `target`,
`model`, `op`, and the row's `pk` (for CREATE, the post-insert value
the DB filled in).

**SQLSTATE → VALIDATION_ERROR mapping (Phase 4).** Constraint
failures from the driver are now mapped into the same per-field
`VALIDATION_ERROR` envelope the in-process validators produce, so the
UI can drop the message next to the input that broke:

| Source                       | Code           |
| ---------------------------- | -------------- |
| Postgres `23505` / SQLite `SQLITE_CONSTRAINT_UNIQUE` / `…_PRIMARYKEY` | `UNIQUE`       |
| Postgres `23503` / SQLite `SQLITE_CONSTRAINT_FOREIGNKEY`             | `FOREIGN_KEY`  |
| Postgres `23502` / SQLite `SQLITE_CONSTRAINT_NOTNULL`                | `REQUIRED`     |
| Postgres `22001` (string truncation)                                 | `LENGTH`       |
| Postgres `22P02` (invalid text representation)                       | `TYPE_MISMATCH`|
| Postgres `23514` / SQLite `SQLITE_CONSTRAINT_CHECK`                  | `REGEX`        |

Unrecognized driver errors still surface as `DATABASE_ERROR` (500).

**Validation codes.** Two new codes on top of Phase 3:

- `UNIQUE` — unique-constraint violation from the database.
- `FOREIGN_KEY` — foreign-key violation from the database.

**UI surfaces (Phase 4).**

- **Tools row.** Above the records table: an op selector + "Show SQL"
  button that fetches the preview and renders it as monospace SQL +
  bind list. Next to it: "Export JSON" / "Export CSV" links that
  point straight at the export endpoint so the browser handles the
  download.
- **Drift dots.** Each model in the sidebar carries a small status
  chip when its live shape doesn't match the schema.
- **Search.** The header's search input fans out to
  `/api/targets/:key/search` on every keystroke; results render in a
  dropdown below the input.
- **Audit overlay.** "Audit" button next to the target switcher
  toggles a 28rem-wide overlay listing recent writes by timestamp.

**Scope notes.**

- Audit log is in-memory only by design — Studio is a local admin
  tool. Restarting the binary clears the buffer.
- Drift inspection talks to `information_schema` (Postgres) and
  `PRAGMA table_info` (SQLite). API-backed targets are reported as
  `unsupported`.
- Export is bounded at 10_000 rows. Larger pulls should use the
  underlying database directly.

### Studio rewrite — Phase 1c + 3 (UI polish + write path)

Studio gains create / update / delete and the UI polish that goes
with it.

**Write API.** Three new endpoints:

```
POST   /api/targets/:key/models/:model/records          -> 201 + row
PATCH  /api/targets/:key/models/:model/records/:pk      -> 200 + row
DELETE /api/targets/:key/models/:model/records/:pk      -> 200 + row
```

All three reject requests against `mode = "ro"` targets with **403
FORBIDDEN**. Writes are wired on all three data sources: Postgres
uses `INSERT/UPDATE/DELETE … RETURNING *` wrapped in `row_to_json` for
type-blind projection; SQLite mirrors the shape with `RETURNING
json_object(...)`; the API source POSTs/PATCHes/DELETEs to the
upstream service's generated `/api/<plural-snake-model>` routes.

The Postgres write path binds typed values based on the field's
declared scalar — `String`/`Uuid`/`Cuid`/`Decimal`/`DateTime`/`Bytes`
as text, `Int` as `i64`, `Float` as `f64`, `Boolean` as `bool`, `Json`
through `sqlx::types::Json`. Anything else (enums) binds as text and
relies on the DB's enum cast.

**Validator pass-through.** A new `validators` module mirrors the
framework's macro-side validators (`@email`, `@length(min:, max:)`,
`@range(min:, max:)`, `@regex("...")`, `@uri`, `@iso4217`) against the
incoming JSON payload before Studio hits the database. Failures
surface as **422 VALIDATION_ERROR** with a structured per-field detail
list the UI can render inline:

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "payload failed validation",
    "fields": [
      { "field": "title", "code": "LENGTH", "message": "field 'title' must be at least 3 characters long" },
      { "field": "authorEmail", "code": "EMAIL", "message": "field 'authorEmail' is not a valid email address" }
    ]
  }
}
```

Validation codes (all `SCREAMING_SNAKE_CASE`): `REQUIRED`,
`TYPE_MISMATCH`, `EMAIL`, `LENGTH`, `RANGE`, `REGEX`, `URI`, `ISO4217`.
The error envelope adds a `fields: []` array — omitted entirely on
non-validation errors so the existing error contract is unchanged.

**UI updates (Phase 1c + 3).**

- **Typed relation picker.** Drawer's relation follow swaps the free
  text input for a dropdown built from the model's `is_relation`
  fields. Labels show `<field> → <target> (<arity>)`.
- **RO / RW badge.** Each model header now displays a small badge
  reflecting the target's mode, so users see at a glance whether
  edits are allowed.
- **Create flow.** RW targets expose a `+ New` button above the
  records table that opens an inline form with one input per writable
  field. Validation errors surface per-field inline; on success the
  table reloads.
- **Edit flow.** RW targets expose an **Edit** button in the drawer
  that turns the field list into editable inputs. **Save** PATCHes
  the row; the response replaces the drawer's view. Per-field
  validation errors appear inline.
- **Delete flow.** RW targets expose a **Delete** button in the
  drawer guarded by a `window.confirm()` prompt. On success the
  drawer clears and the table reloads.
- **Pretty JSON viewer.** Object/array cell values in the drawer now
  render through `serde_json::to_string_pretty`.

**Error codes.** Two additions on top of Phase 1b's set:

- `FORBIDDEN` (403) — target is read-only.
- `VALIDATION_ERROR` (422) — payload-level validation failure with
  per-field detail.

The earlier (`BAD_REQUEST`) code is now reserved for malformed request
bodies (e.g. invalid JSON); validation errors get their own code so
the UI can route them into per-field error displays.

#### Scope notes

- Validators run before the DB. Constraint-level failures (UNIQUE,
  NOT NULL, CHECK, type mismatch beyond what we catch) still surface
  as `500 DATABASE_ERROR` with the underlying driver message; mapping
  SQLSTATE / SQLite extended codes to friendlier validation
  envelopes is Phase 4.
- The UI's create / edit form is a single text-input per field; typed
  pickers for enums and rich editors for JSON / DateTime / Decimal
  are Phase 1d.
- API targets accept writes and forward them to the upstream's REST
  routes verbatim. The upstream's own policy/auth enforces what's
  actually allowed.

### Studio rewrite — Phase 2 (`studio eject` + bundled UI)

Two things land in this phase. Both are about making Studio
distributable rather than dev-only.

**`cratestack studio eject --out <dir>`** writes a writable copy of
Studio's Leptos+Trunk UI into the target directory: `Cargo.toml`,
`Trunk.toml`, `index.html`, `src/{lib,api,app,types}.rs`, and a
purpose-built `README.md` that explains the standalone build flow.
Generated artifacts (`dist/`, `target/`, `Cargo.lock`) are skipped so
the eject output is a clean checkout. The UI tree is embedded into the
framework binary at compile time via `include_dir!`, so eject is a
single-step copy with no template substitution to drift.

```
cratestack studio eject --out ./fork
# wrote 9 files; cd ./fork && trunk serve
```

`--force` lets you overwrite an existing non-empty directory; without
it, eject refuses to clobber.

**`embed-ui` cargo feature** bundles the Trunk release build into the
Studio binary via `rust-embed`. Build flow:

```bash
cd crates/cratestack-studio/ui && trunk build --release
cargo build -p cratestack-cli --bin cratestack \
  --features cratestack-studio/embed-ui
```

With the feature on, `cratestack studio run` serves the SPA at `/`,
keeps the JSON API mounted at `/api/*`, and falls back to `index.html`
for unknown paths so the browser's client-side routing works. With
the feature off (the default), `/` still serves the Phase 1b stub
explainer so the binary stays minimal for dev.

Wiring: API routes are mounted before the UI routes, so any future
overlap resolves in favor of the JSON surface. The bundled-UI tests
explicitly assert that `/api/targets` still hits the JSON handler
when the SPA fallback is in play.

#### Crate / module changes

- `cratestack-studio` gains `mod eject` (with `eject()`, `EjectOptions`, `EjectError`, `EjectReport`) and an `embed-ui`-gated `mod ui_assets`.
- `cratestack-studio-generator` is now a thin re-export of `cratestack_studio::eject` so the CLI's existing import surface keeps working. New code should depend on `cratestack-studio` directly.
- `cratestack-cli`'s `studio eject` subcommand gains `--force` and now prints the eject report (file count + next-steps hint).
- New workspace deps: `include_dir = "0.7"`, `rust-embed = "8"` (used only when the `embed-ui` feature is on).

#### Scope notes

- The `embed-ui` feature requires a Trunk release build to have produced `crates/cratestack-studio/ui/dist/`. Building the feature without that tree fails fast at the embed step.
- Eject's output README points users at the framework's docs for upstream upgrades. There's no automated re-eject path — a forked UI is a fork.

### Studio rewrite — Phase 1b (read API completions + Leptos UI)

Phase 1b finishes the read story. SQLite targets are now a first-class
driver, the `@relation` traversal endpoint is wired, the API-backed
list/get path talks to deployed CrateStack services, and a Leptos+Trunk
web UI consumes all of it from the browser.

**SQLite via rusqlite.** A new `data::sqlite::SqliteSource` opens a
SQLite connection per target and projects rows through SQLite's
`json_object(...)` so the rest of the pipeline stays identical to the
Postgres path. Studio doesn't use `sqlx-sqlite` because the workspace's
`rusqlite 0.39 → libsqlite3-sys 0.37` pin conflicts with sqlx-sqlite's;
the rusqlite-based source has no such conflict. `[target.db]` URLs
accept `sqlite:`, `sqlite://`, `sqlite::memory:`, and bare file paths.

**Relation follow.** New endpoint
`GET /api/targets/:key/models/:m/records/:pk/rel/:field`. The
resolver reads `@relation(fields: [...], references: [...])` symmetrically
on both ends of a relation: the target is the field's declared type,
the source row's `fields[0]` supplies the bound value, and we filter
the target table on `references[0]`. List-arity fields return a paginated
page; Required-arity fields return a single optional row. Both sides
of the relation must declare `@relation` (which is what the CrateStack
parser already enforces).

**API list/get.** `data::api::ApiSource` now talks to a deployed
CrateStack service over the same REST routes the generated TypeScript
and Dart clients use: `GET <base>/api/<plural-snake-model>` for list,
`GET <base>/api/<plural-snake-model>/{id}` for find_unique. Studio
maps its cursor abstraction onto the upstream's offset/limit pagination
by encoding the next offset as the opaque cursor string. Auth headers
follow `[target.api.auth]` (`bearer { token = … }` or `header { name,
value }`). Relation follow against API targets returns `UNSUPPORTED` —
the generated REST surface doesn't expose arbitrary column filters.

**Dev CORS.** `[workspace] cors_dev = true` (the default) layers a
permissive CORS layer on the router so a Trunk dev server on
`localhost:8080` can talk to the Studio backend on `localhost:7878`.
Set `cors_dev = false` when binding to a wider interface.

**Leptos UI.** New `crates/cratestack-studio/ui/` crate — a Leptos
CSR app built by Trunk, intentionally excluded from the workspace so
`cargo check --workspace` doesn't pull in the `wasm32-unknown-unknown`
toolchain. Surface:

- Header with workspace name and target switcher (shows mode + db/api capability).
- Left sidebar listing the selected target's models.
- Records table with cursor-based pagination (previous/next).
- Record drawer with a per-field view, a relation-follow input, and a
  "Copy Rust query" button that writes the find_unique snippet to the
  system clipboard.

Run locally with `cratestack studio run` in one terminal and
`trunk serve` in `crates/cratestack-studio/ui/` in another; Trunk's
proxy forwards `/api/*` to the backend on port 7878.

**Error envelope additions.** Two new stable codes: `UNKNOWN_FIELD`
(unknown field name on relation follow, 404) and `NOT_A_RELATION`
(field exists but isn't a relation, 400). `INTERNAL_ERROR` is reserved
for blocking-task panics during the SQLite path.

#### Scope notes

- Relation follow is read-only and supports the two common shapes
  (outgoing 1-1 / many-1, inbound 1-many). Many-to-many through a
  junction table returns `UNSUPPORTED`.
- The UI's relation follow currently takes the field name as a free
  text input — a typed dropdown lands in Phase 1c once the UI threads
  the per-model relation field list down to the drawer.
- The Studio binary still ships without the UI compiled in. Phase 2's
  `studio eject` writes the UI's sources to a writable workspace; Phase
  2 / 3 also adds the `rust-embed` bundle for single-binary distribution.

### Studio rewrite — Phase 1a (read API)

The studio gains a real backend. `cratestack studio run` now parses
each target's `.cstack`, opens a sqlx Postgres pool (when the target
has a `[target.db]` block), and serves six read endpoints:

```
GET /api/targets
GET /api/targets/:key/schema
GET /api/targets/:key/models
GET /api/targets/:key/models/:model/records?cursor=…&limit=…
GET /api/targets/:key/models/:model/records/:pk
GET /api/targets/:key/models/:model/snippet?pk=…
```

`/snippet` returns a Rust `find_unique` call against the macro
delegate so you can paste it into a service crate. Primary-key
literals are typed: `String`/`Cuid`/`Uuid`/`Decimal` IDs render as
`"…".to_owned()`, `Int` IDs as `42_i64`.

Pagination is cursor-based on the model's `@id`. Cursors are bound as
text and cast in SQL (`$1::bigint` for Int PKs, no cast for text-shaped
PKs) so the Rust side stays blind to column types. Row projection uses
Postgres's `row_to_json(t.*)` over the model's scalar columns, which
keeps the dynamic decode path off the type-OID treadmill.

Studio now reads `env:NAME` and `file:PATH` references in
`studio.toml`. `target.db.url` and `target.api.auth.{token,value}` are
resolved at boot; unset env vars and missing files surface a load-time
error that names the bad config field.

API responses use a uniform error envelope —
`{"error": {"code": "…", "message": "…"}}` — with stable codes
(`UNKNOWN_TARGET`, `UNKNOWN_MODEL`, `NO_PRIMARY_KEY`,
`INVALID_PRIMARY_KEY`, `UNSUPPORTED`, `DATABASE_ERROR`,
`UPSTREAM_ERROR`).

#### Scope limits

- **Postgres only.** The workspace currently pins `rusqlite` (used by
  `cratestack-rusqlite` and `cratestack-client-store-sqlite`) against
  `libsqlite3-sys` 0.37, which conflicts with `sqlx-sqlite`'s pin.
  Phase 1b adds an alternate SQLite path that uses `rusqlite` directly
  so the two crates can coexist.
- **No relation follow yet.** `/api/targets/:key/models/:m/records/:pk/rel/:f`
  lands in Phase 1b alongside the UI.
- **API-only targets return 501 on list/get.** Schema and snippet
  endpoints work because they read the parsed schema, not the upstream;
  list/get against `[target.api]` targets is wired in Phase 1b.
- **Primary-key types.** Phase 1a accepts `String`, `Cuid`, `Uuid`,
  `Decimal`, and `Int`. Other PK types (`DateTime`, `Bytes`, etc.)
  return `UNSUPPORTED`.

### Studio rewrite — Phase 0 (breaking)

The Jinja-templated `cratestack generate-studio` scaffold is removed. In its
place is a new crate, `cratestack-studio`, and a new CLI surface,
`cratestack studio`, with three subcommands:

```sh
cratestack studio init                  # writes ./studio.toml
cratestack studio run                   # binds 127.0.0.1:7878 by default
cratestack studio eject --out ./out     # Phase 2 — currently returns NotImplemented
```

The studio now reads a workspace file (`studio.toml`) that lists one or
more `[[target]]` blocks. Each target points at a `.cstack` schema and
declares how the studio reaches its data: a `[target.db]` block for
direct sqlx connections, a `[target.api]` block for a deployed
cratestack service, or both. A target with neither channel is rejected
at load time.

Phase 0 only ships the skeleton: config loader, target validation, and
an Axum server that exposes `/` (stub page) and `/api/health` (workspace
+ target summary). Schema introspection, record browsing, mutations, and
the Leptos UI follow in Phases 1-4.

`cratestack-studio-generator` is now a transitional shim. Its 0.3.x
public API (`generate_package`, `StudioGeneratorConfig`,
`StudioGeneratorContext`, `StudioProfile`, `GeneratedStudioFile`,
`GeneratedStudioPackage`) is gone; the only remaining surface is a
placeholder `eject()` that will, in Phase 2, copy `cratestack-studio`'s
own sources into an output directory for users who want to fork the UI.

Migration for existing `generate-studio` users: run `cratestack studio
init` to seed a `studio.toml`, fill in your schemas and connection
strings, then `cratestack studio run`. There is no automated migration
of the 0.3.x multi-crate output — it was generated code and should be
regenerated from the new shape.

### RPC transport (v1): `transport rpc` as an alternative to REST

A `.cstack` schema now picks exactly one generation style via a
top-level `transport rest|rpc` directive (default `rest`, so existing
schemas parse unchanged) — one binding's worth of public surface, not
both. Under `transport rpc`, every CRUD verb per model and every
procedure gets an op id (`model.User.list`, `procedure.publishPost`),
dispatched over two endpoints instead of a route per model/verb:

```
POST /rpc/:op_id       # unary
POST /rpc/batch        # server may parallelize; no in-batch dependencies,
                        # no transactional mode — use a procedure or two
                        # round trips for composite ops
```

The op id lives in the URL rather than the request body — operationally
honest, since nginx/CDN/HTTP tracing all work per-route that way — and
client codegen branches on the schema's transport style, so a generated
SDK ships one client's worth of code, not both (#20–#24, examples in
#27). Error responses use gRPC-style codes in a stable `RpcErrorBody`
shape (#23). Streaming (`application/cbor-seq`) needed no code change
at all: content negotiation on the existing sequence encoder already
handled it (#24).

**Deferred:** the WebSocket binding and `@@subscribe`-driven
subscriptions from the original design are not part of this release —
today's audit/event-bus consumers are server-to-server and don't need
a WS channel, so this is picked up when a concrete consumer needs it,
not before (#25).

### ORM additions

Landed alongside the RPC work above, independent of transport style:

- **Transaction-aware writes**: `.for_update()` and `update_many` join
  the existing write surface, both participating correctly in an
  ambient transaction (#26).
- **Composite-key upsert** and **`find_unique` detail policy** support
  (#28).
- **Nullable-OR filter** and a **`COALESCE` multi-column filter** for
  querying across nullable columns without hand-written SQL (#29).
- **`aggregate`**, **`delete_many`**, and `NULLS FIRST`/`NULLS LAST`
  ordering (#37).
- **JSONB filter operators** — `json_has_key` + `json_get_text` (#42).
- **`FindMany.include()`** — to-one relation side-loading in a single
  round trip (#44).
- **PostGIS spatial filters** — `covers_geography` + `dwithin_geography`
  (#48).
- **Column projection** — `find_*.select(...)` returning a typed
  `Projection<T>` instead of the full model (#51).
- **`ProjectedFindMany.run_in_tx`**, plus an `enum` `Default` fix (#55).

### Client streaming (cbor-seq)

The generated clients gain first-class consumers for the streaming
transport introduced above:

- **Rust**: `RpcClient::call_streaming` returns an `mpsc::Receiver`,
  fed by a `cbor-seq` streaming decoder (#30, #34). Also gains a typed
  batch API — same method, two consumption modes (#53).
- **Dart**: `CborSeqStreamTransformer` + a decoder-handle contract
  (#43), and an `rpc_call_streamed` FFI entrypoint for
  `/rpc/{op_id}` (#39).
- **Flutter**: `execute_streamed` FFI shim over the cbor-seq path
  (#33), and `FlutterCborSeqDecoder` for `dio`-driven streaming (#40).
- **Codegen**: client generators now branch on `Schema.transport` to
  emit RPC clients where the schema calls for them (#32, #50).

### Workspace-wide 200-LoC refactor

Every `.rs` file under `crates/*/src/` is now ≤200 LoC, landed across 16
PRs (#57–#76). No public API changes — all splits preserve the crate
surface via `pub use` re-exports. The major rewrites:

- `cratestack-sqlx` and `cratestack-rusqlite` delegate / render / batch /
  value modules split into focused submodules
- `cratestack-axum` idempotency, rpc, transport, ratelimit, headers,
  codec all broken into per-concern files
- `cratestack-macros` four giants split (include / model / axum /
  relation), medium files re-grouped
- `cratestack-client-{dart,rust,typescript,flutter}` `lib.rs` split into
  per-concern modules (largest: client-rust at 2369 → 18 submodules)
- `cratestack-parser` 880-line `parse.rs`, 1086-line `validate.rs`, and
  1336-line `tests.rs` split per topic
- `cratestack-lsp` `main.rs` (1273 LoC) split into 11 submodules
- `cratestack-client-dart` README and rpc-runtime jinja templates split
  via `{% include %}` fragments (loader sets
  `set_keep_trailing_newline(true)` for byte-identical output)
- Inline `#[cfg(test)] mod tests` blocks throughout the workspace
  extracted into `tests_<topic>.rs` siblings

### README fixups

Four crate READMEs (`cratestack-axum`, `cratestack-sqlx`,
`cratestack-client-rust`, `cratestack-parser`) still referenced the
pre-0.3.0 macro names (`include_schema!`,
`include_client_macro!`) — updated to the current
`include_server_schema!` / `include_client_schema!`. The `client-rust`
README's two duplicate sections (one per old macro) collapse into one.

### Other

Test-support scaffolding (`tests/support/pg.rs`) covering
compose/testcontainers/skip backend selection for PG-backed integration
tests (#19), and an internal `cratestack-axum` module split
(codec/transport/headers/query) with deduped RPC helpers (#31).

## 0.3.2 (2026-05-14)

### Batch primitives — tRPC-style per-item envelope

Five new ORM methods on every model delegate, on both the sqlx (server) and rusqlite (embedded) backends:

```rust
cool.account().batch_get(vec![1, 2, 999]).run(&ctx).await?
cool.account().batch_create(vec![input_a, input_b]).run(&ctx).await?
cool.account().batch_update(vec![(1, patch_a, Some(0)), (2, patch_b, None)]).run(&ctx).await?
cool.account().batch_delete(vec![1, 2]).run(&ctx).await?
cool.account().batch_upsert(vec![input_a, input_b]).run(&ctx).await?
```

Every batch call returns `Result<BatchResponse<M>, CoolError>`. The outer `Result` is reserved for whole-batch infrastructure failures (size cap exceeded, duplicate input keys, DB connection lost). Per-item failures (validation, policy denial, NotFound, stale `if_match`, PK conflict) ride inside the envelope as `BatchItemStatus::Error { error: BatchItemError { code, message } }`, with `index` preserved so callers can pair results back to their input position.

```json
{
  "results": [
    { "index": 0, "status": "ok", "value": { ... } },
    { "index": 1, "status": "error", "error": { "code": "POLICY_DENIED", "message": "..." } },
    { "index": 2, "status": "ok", "value": { ... } }
  ],
  "summary": { "total": 3, "ok": 2, "err": 1 }
}
```

### Transactional model

- **Two single-statement ops** (`batch_get`, `batch_delete`) issue one `SELECT … WHERE pk IN (…)` or `DELETE … WHERE pk IN (…) RETURNING …`. Policy predicates merge into the WHERE; rows that don't match (because they don't exist, were already tombstoned, or the read/delete policy hid them) surface as per-item `NOT_FOUND`.
- **Three savepointed ops** (`batch_create`, `batch_update`, `batch_upsert`) run all items in one outer transaction with a per-item `SAVEPOINT`. A per-item failure rolls back its savepoint only — successful items in the same batch still commit. The audit log records one row per successful item, with the outer commit timestamp; failed items leave no audit row, no event outbox entry, no row mutation.
- The cap is `1000` items per call (`cratestack_core::BATCH_MAX_ITEMS`); over-sized batches are rejected before any SQL runs.

### Loud-fail on duplicate input keys

The framework refuses batches with duplicate primary keys at the outer guard, returning `CoolError::Validation` (or `RusqliteError::DuplicateBatchKey` on the embedded side) with the indices of the first and duplicate occurrences. Silently collapsing duplicates would break the per-item `index` mapping the envelope promises and hide caller bugs; we want callers to dedupe at the boundary they own.

Detection runs on:

- the PK list for `batch_get` / `batch_delete`
- the per-item PK in `batch_update` items
- `UpsertModelInput::primary_key_value()` for `batch_upsert`

`batch_create` skips the check — `CreateModelInput` doesn't expose the PK generically, and duplicate client-supplied PKs already trip the database's unique constraint per-item (surfacing as `CoolError::Conflict` in that item's envelope, while the rest of the batch commits cleanly via savepoint isolation).

### Internal

- New types in `cratestack-core`: `BatchItemResult<T>`, `BatchItemStatus<T>`, `BatchItemError`, `BatchSummary`, `BatchResponse<T>`, `BatchRequest<I>`, `BATCH_MAX_ITEMS`, `find_duplicate_position`.
- New trait in `cratestack-sql`: `ModelPrimaryKey<PK>`, emitted by the macro on every generated model struct. Used by `batch_get` / `batch_delete` to pair returned rows back to their input position.
- New helper in `cratestack-sql`: `find_duplicate_sql_value` for upsert-side dedup, since `SqlValue::Float` / `SqlValue::Decimal` don't admit a sound `Hash` impl.
- New `RusqliteError` variants: `BatchTooLarge { actual, maximum }` and `DuplicateBatchKey { first, duplicate }`.

### Worked example

The `examples/embedded-cli` notes app gains three batch subcommands that walk through the envelope in real terminal output:

```text
$ notes import bulk-load.json
OK  [0] 11111111-…  first
OK  [1] 22222222-…  second
summary: 2 total, 2 ok, 0 err

$ notes bulk-done 11111111-… 99999999-…
OK  [0] 11111111-…  first
ERR [1] NOT_FOUND: no row matched
summary: 2 total, 1 ok, 1 err
```

- `notes import <file.json>` — `batch_upsert` over a JSON file; replays converge.
- `notes bulk-done <id> [id...]` — `batch_update` to mark complete.
- `notes bulk-delete <id> [id...]` — `batch_delete`.

### Deferred

- **Auto-generated `POST /<model>/batch-*` axum routes**: the wire envelope types (`BatchRequest<I>` / `BatchResponse<T>`) are stable in `cratestack-core` so apps can hand-roll a thin handler against the ORM today. Macro-driven route emission per model lands in a follow-up.
- **Per-item `if_match` on the embedded `batch_update`**: the rusqlite layer doesn't enforce `@version` for single rows either; consistency over surprise.

## 0.3.1 (2026-05-14)

### New crate: `cratestack-migrate` — schema diff + migration generator

Implements ADR-0004, the *authoring* side of the migration story: a new
`cratestack-migrate` crate diffs a parsed `.cstack` against a committed
snapshot and emits per-backend SQL migrations. The runner (already in
`cratestack-sqlx`) is unchanged — it consumes the generated SQL
identically to hand-written migrations.

```
cratestack migrate diff --schema schema.cstack --out-dir migrations --backend both --name <slug>
```

Per-backend output lives under
`migrations/<postgres|sqlite>/<timestamp>_<slug>/` as `up.sql` /
`down.sql`, alongside a committed `schema.snapshot.json`. The diff
engine produces a backend-agnostic op list ordered by DDL dependencies
(enums → renames → drops → creates → adds → check constraints → enum
drops), covering table/column add-drop, indexes (from `@unique`),
column type/nullability/default changes, renames (`@@rename` /
`@rename`), enums, and check constraints (`@db_enforce` promotion of
`@range` / `@length` / `@iso4217`).

**Destructiveness gating.** Every op is classified Safe / Lossy /
Blocking; `--allow-destructive` is required to write any migration
containing a lossy op, and `down.sql` for a lossy migration is an
explicit error stub (`RAISE EXCEPTION` / `RAISE(FAIL, ...)`) rather
than a real rollback — matching the runner's irreversible-by-default
posture (#16).

**Deferred (intentional):** `migrate verify` and `migrate drift` need
ephemeral DB spawning and live introspection, each with its own CI
footprint; view-block IR ops need the `view` block itself (ADR-0003)
built out first; `DropEnumVariant` needs a Postgres swap-dance plus a
backfill plan for referencing rows.

### Examples, docs, and CI

- Pure-Rust example set covering all three 0.3.0 macros side by side
  (#10), and a root README rewrite for the macro split (#11).
- In-browser embedded SQLite example plus a wasm32 facade refactor
  (#12); `embedded-expo` × `embedded-flutter` × `tauri-native` (#14);
  `embedded-daemon` + `embedded-webhook` showing async I/O layered
  around the sync `ModelDelegate` (#15).
- CI's rustdoc job now restricts to the framework crates so it doesn't
  pull in GTK transitively via the Tauri examples (#13).

### Upsert primitive

New `.upsert(input)` on every model whose `@id` is client-supplied (i.e. has no `@default(...)`). Backed by `INSERT … ON CONFLICT (<pk>) DO UPDATE …`. Available on both the sqlx (server) and rusqlite (embedded) backends.

```rust
// Server (sqlx) — both create and update policies enforced, event/audit
// driven off a SELECT … FOR UPDATE probe inside the same transaction.
cool.tag().upsert(CreateTagInput { id, label }).run(&ctx).await?;

// Embedded (rusqlite) — single statement, no audit/event machinery.
delegate.upsert(CreateTagInput { id, label }).run()?;
```

Models with server-generated PKs (`@id @default(cuid())`, etc.) get **no** `UpsertModelInput` impl — calling `.upsert(...)` on them is a compile error rather than a runtime "not supported." Unique-key (non-PK) conflict targets are deferred.

Semantics:

- **Both create and update policies must allow the call** — evaluated at call time, before the runtime knows which branch will fire. Pre-flighting a read to pick a policy slot would leak row existence to the caller.
- **`@version` columns are bumped server-side** on the update branch (`<table>.<col> + 1`). `if_match` is not supported on upsert — use `.update(...).if_match(...)` if you need it.
- **Soft-deleted rows act as "no row"**: the INSERT branch will then trip the PK uniqueness constraint, which is the right outcome (refuse to silently revive a tombstone).
- **Event / audit fan-out** picks `Created` vs `Updated` based on whether the `SELECT FOR UPDATE` probe saw a row — not Postgres `xmax`, so the rusqlite mirror stays trivial.
- **Auth-derived defaults (`@default(auth().*)`) are excluded from the update branch** — they're identity bindings, and clobbering them on update would turn upsert into "take ownership of any row I name." The full list of columns the update branch is allowed to overwrite is exposed on `ModelDescriptor::upsert_update_columns`.

### Internal

- `ModelDescriptor::new(...)` gained one trailing argument (`upsert_update_columns`). Schemas built through `include_*_schema!` are unaffected; hand-rolled descriptors need the extra `&[]`.

## 0.3.0 (2026-05-13)

### Headline: three macros, one schema, no dead weight

The single `include_schema!` macro is gone. In its place are three role-specific macros that emit only what each deployment needs. No more mobile apps transitively pulling `sqlx` they don't use; no more server builds carrying `rusqlite` for nothing.

```rust
// Server (Postgres via sqlx) — full ORM, axum routes, procedures, events
include_server_schema!("schema.cstack", db = Postgres);

// Embedded (rusqlite) — works native and on `wasm32-unknown-unknown` via OPFS
include_embedded_schema!("schema.cstack");

// HTTP client — model/input stubs, procedure clients, zero DB
include_client_schema!("schema.cstack");
```

The split is **strict**: `include_server_schema!` does not emit anything rusqlite-related, and `include_embedded_schema!` does not emit anything sqlx-related. Each deployment shape pays only for its own surface.

### Breaking changes

- **Removed `include_schema!`.** Migrate server callers to `include_server_schema!("…", db = Postgres)`. Migrate sqlite/embedded callers to `include_embedded_schema!("…")`.
- **Renamed `include_client_macro!` → `include_client_schema!`** for naming consistency with the new macros.
- **`include_server_schema!` requires a `db = …` argument.** Today only `db = Postgres` is accepted; the parser is wired so adding `MySql` / `Sqlite`-via-sqlx in a future release is non-breaking at call sites that already pass `db = Postgres`.
- **`include_embedded_schema!` emits `::cratestack_rusqlite::*` paths**, not `::cratestack::*`. Embedded consumers should list `cratestack-rusqlite` and `cratestack-macros` directly in their `Cargo.toml`; the heavyweight `cratestack` facade is no longer required for an embedded build.
- **Deleted the `cratestack-sqlite-wasm` crate.** Originally written as a separate wasm32 backend; superseded by `rusqlite 0.39`, which targets wasm32 transparently via `sqlite-wasm-rs`. Use `cratestack-rusqlite` with the `wasm32-unknown-unknown` target and the new `cratestack_rusqlite::opfs::install_opfs_vfs()` helper (must run inside a Dedicated Worker).
- **Bumped `rusqlite` to `0.39`** (from the previously-resolved `0.32`). Internal `u64` columns now require the `fallible_uint` feature (enabled by default in our workspace pin).
- **Internal: `cratestack-sqlx` migrated off the `sqlx` umbrella crate** to depend on `sqlx-core` + `sqlx-postgres` directly. The umbrella's `sqlx-sqlite` leaked into the resolve graph and conflicted with `rusqlite 0.39`'s `libsqlite3-sys 0.37`. Public surface stays as `cratestack::sqlx::*` via a compatibility shim in `cratestack-sqlx` — no consumer changes required for code that referenced the facade path.
- **Internal: `cratestack-lsp` migrated from unmaintained `tower-lsp 0.20` to `tower-lsp-server 0.23`.** The fork ports the same crate to native `async fn` in traits (Rust 1.75+), drops `#[async_trait]` attributes, renames `lsp_types` → `ls_types`, and switches `Url` → `Uri` (from `fluent-uri`). User-facing LSP behavior unchanged.

### Migration cheat sheet

| Before | After |
|---|---|
| `include_schema!("schema.cstack");` (server context) | `include_server_schema!("schema.cstack", db = Postgres);` |
| `include_schema!("schema.cstack");` (sqlite/mobile context) | `include_embedded_schema!("schema.cstack");` |
| `include_client_macro!("schema.cstack");` | `include_client_schema!("schema.cstack");` |
| `use cratestack::include_schema;` | `use cratestack::{include_server_schema, include_embedded_schema, include_client_schema};` (pick what you need) |

### New features

- **In-browser SQLite ORM.** `cratestack-rusqlite` now compiles to `wasm32-unknown-unknown`. The new `cratestack_rusqlite::opfs::install_opfs_vfs(&OpfsOptions::default()).await?` installs the OPFS SAH-pool VFS so `RusqliteRuntime::open(filename)` persists across page reloads. Must run inside a Dedicated Worker.
- **Single SQLite backend everywhere.** The same `cratestack-rusqlite` crate now serves mobile (libsqlite3), desktop (libsqlite3), and browser (OPFS via `sqlite-wasm-rs`). One code path, one API.

### Known follow-ups

- `@@audit` and `@@emit` directives are currently no-ops in `include_embedded_schema!`. The local-journal / local-event-bus implementations need their own design pass (sync engine, conflict resolution); they will land in a follow-up release.
- `cratestack-sqlx` could lose its `cratestack::sqlx::*` compatibility shim once we've validated nobody depends on it externally. Tracked as a 0.4.0 cleanup.
- Multi-DB support (MySQL, SQLite-via-sqlx) for `include_server_schema!` — the `db = …` arg parser is ready; the codegen needs the abstraction.

## 0.2.3 (2026-05-12)

`cratestack-redis` gains **`RedisRateLimitStore`**, enforcing a single
global token-bucket per key across replicas via one atomic
read-refill-decrement-write Lua script; bucket state lives at
`<prefix>:rl:<sha256(key)>` with a self-refreshing `EXPIRE` so idle
keys evict themselves. Skips its live-Redis integration tests cleanly
when no Redis is configured, matching the sqlx-store test pattern
(#7).

## 0.2.2 (2026-05-12)

Docs-only: every crate README rewritten against its actual API
surface rather than aspirational/stale examples (#6).

## 0.2.1 (2026-05-12)

### New crate: `cratestack-rusqlite` — the embedded SQLite backend

The embedded backend's real implementation: `ddl`, `delegate`,
`render`, `row`, `runtime`, and `value` modules, plus an `ffi` layer
for non-Rust embedders (#4).

### New crate: `cratestack-redis` — `RedisIdempotencyStore`

A server-side Redis-backed idempotency store, sibling to
`cratestack-sqlx`'s `SqlxIdempotencyStore`, for multi-replica
deployments that need shared idempotency state across instances rather
than per-process memory. Atomicity comes from three Lua scripts
(`reserve_or_fetch`, `complete`, `release`) run via `EVALSHA` with
`NOSCRIPT` fallback; reservation lifetimes are driven by `PEXPIREAT`,
and token rotation on reclaim plus token/status guards inside
`complete`/`release` stop a stale handler from poisoning a newer
reservation. State lives in one Redis hash per `(principal, key)` at
`<prefix>:idem:<sha256(principal || 0x00 || key)>` (#5).

## 0.2.0 (2026-05-12)

The first version actually published to crates.io. (`v0.1.0` was never
published under that number — see the note at the bottom of this file.)

### Banking-readiness: a three-phase hardening pass

The framework's first push from e-commerce-production-grade toward
banking-grade, landed as one large merge (#2, #3):

- **Phase 1 — correctness & money.** `Decimal` scalar
  (feature-flagged `decimal-rust-decimal` / `decimal-bigdecimal`, the
  latter still a `compile_error!` stub), error redaction (4xx messages
  public, 5xx detail-only), optimistic locking (`@version` +
  If-Match/ETag), schema validation attributes (`@length` / `@range` /
  `@regex` / `@email` / `@uri` / `@iso4217`), idempotency
  (`IdempotencyLayer` + `SqlxIdempotencyStore`, opt-in via
  `Router::layer(...)`, not auto-wired into macro-generated routers).
- **Phase 2 — compliance & integrity.** Audit log (`@@audit`,
  before/after snapshots), field-level policy (`@readonly` /
  `@server_only`), transaction isolation (`@isolation("...")`),
  PII/data classification (`@pii` / `@sensitive`), correlation IDs
  (traceparent propagation).
- **Phase 3 — hardening & ecosystem.** HMAC signed envelope
  (`COSE_Sign1`/ES256 trait-ready, not yet implemented), rate limiting
  (`RateLimitLayer`), anti-replay nonce store, API versioning
  (`@api_version`), soft-delete (`@@soft_delete`, GC left as a
  follow-up), FIPS crypto feature flag (a real FIPS certification
  still needs a vendor-validated libcrypto), and a migration engine
  (`cratestack_migrations` table + `apply_pending` — schema-diff-driven
  generation was explicitly out of scope at this point; that landed
  later as `cratestack-migrate`, see `v0.3.1`).

**Known-outstanding at this point:** `IdempotencyLayer` still isn't
auto-wired into macro-generated routers by default; `RedisNonceStore`
doesn't exist yet (`RedisIdempotencyStore` and `RedisRateLimitStore`
land in `v0.2.1`/`v0.2.3`, right after this); `COSE_Sign1` has no real
ES256/EdDSA signing behind it yet, trait surface only.

### CLI, mixins, and the TypeScript client

- **`cratestack` CLI** for schema tooling, the framework's first
  command-line surface.
- **Mixin support** — `@use` composes shared field groups into
  `.cstack` models.
- **Generated TypeScript client** gains TanStack Query hooks, and a
  Rust **client-only macro** (the predecessor to what later became
  `include_client_schema!` in the `v0.3.0` three-macro split) for
  generated Rust clients, plus request-authorization support.
- **`cratestack-client-store-redis`** — a Redis-backed client-side
  state store.
- Backend-to-backend client guidance defaults to the CBOR codec and
  clarifies OAuth2 endpoint handling.

### Public release housekeeping

The repo's public GitHub Pages docs deployment (custom domain +
rustdoc root redirect) is fixed, and the codebase is scrubbed of
internal-only references from before the project's public rename —
this is the release the rest of this changelog's history starts
counting from.

---

`v0.1.0` doesn't have a section above because it was never published —
no crates.io release, no tag. It was the version number in `Cargo.toml`
during the project's pre-public "extraction" work (renaming from an
internal codename, stripping internal-only references, standing up the
CLI/docs/public-release plumbing) before the very first real release,
which shipped as `v0.2.0` above instead.
