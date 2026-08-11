# ADR 0016: How Far the Store SPI Should Reach

## Status

Proposed

## Date

2026-08-08

Context doc: [docs/design/layering.md](../design/layering.md)

## Context

"Store SPI" names two unrelated things in this workspace, and the question
only becomes answerable once they are separated.

**The operational store traits** (L1). Three traits, **five methods total**:
`IdempotencyStore` (`core/src/store/idempotency.rs`, 73 lines —
`reserve_or_fetch` / `complete` / `release`), `RateLimitStore`
(`core/src/store/ratelimit.rs`, 91 lines — `consume`), and `AuditSink`
(`core/src/audit.rs:81` — `record`). Two of the three are genuine runtime
seams with shipped alternatives: `SqlxIdempotencyStore`
(`sqlx/src/idempotency.rs:58`) vs `RedisIdempotencyStore`
(`redis/src/idempotency/trait_impl.rs:15`); `InMemoryRateLimitStore`
(`axum/src/ratelimit/store.rs:35`) vs `RedisRateLimitStore`
(`redis/src/ratelimit/trait_impl.rs:14`). The third, `AuditSink`
(`NoopAuditSink` at `audit.rs:92`, `MulticastAuditSink` at `:113`), has no
caller anywhere: `record()` is invoked only by `MulticastAuditSink` fanning
out to its own children (`audit.rs:117`), and the generated
`CratestackBuilder` (`macros/src/include/server/runtime/postgres.rs:46-48`)
has no method to install one. `extensions.md` §2 layer 3 already
generalises this family as the framework's standing pattern — while
simultaneously overstating it, describing "three interchangeable backends"
for `RateLimitStore` where there are two.

**The descriptor SPI** (also L1). `ReadSource<M, PK>` (12 required + 2
defaulted methods) and its supertrait `WriteSource<M, PK>` (14 more) in
`sql/src/descriptor/read_source.rs`. These abstract *descriptor shape* —
model vs view — not storage backend. Nothing here is swappable at runtime;
both are implemented by compile-time `&'static` descriptors.

The Spring-Data question is whether a third thing should exist: a
`Repository<M, PK>` / `Store<M, PK>` seam that makes a model's persistence
backend-swappable, the way Spring Data's repository abstraction does. Six
measurements bear on it, all taken against `origin/main` (`08fbb7e`):

1. **L1 covers ~15% of the mass it would have to cover.** `cratestack-sql`
   is 2,799 lines of `src/`; the two adapters it serves are 10,907
   (`cratestack-sqlx`) and 4,828 (`cratestack-rusqlite`). Add
   `cratestack-migrate`'s 9,162, whose `src/introspect/` is 972 lines
   across twelve files, **eleven of them under `introspect/postgres/`**
   with no SQLite counterpart — `cratestack migrate diff` against a live
   database is Postgres-only, and a third backend inherits that hole. Query
   planning is shared; execution, transactions, audit, idempotency DDL,
   migrations and introspection are not.

2. **`Dialect` has one method and its own doc states the rule.**
   `sql/src/dialect.rs:14-18`: `write_placeholder`, plus the comment "Kept
   deliberately narrow — adding methods here forces every backend to
   implement them, which is the wrong default. New dialect-specific quirks
   should live in the backend's own renderer until at least two backends
   agree on the shape." That is already the answer to "should L1 grow", and
   it was written by someone who had the SQLite backend in front of them.

3. **The last widening has not yet paid for itself.** `WriteSource` has one
   impl (`sql/src/descriptor/model_impls.rs:62`) and **zero consumers**: a
   workspace-wide search for `dyn WriteSource` or a `WriteSource` bound
   returns nothing outside the trait's own definition, its doc comments,
   two crate READMEs, and three facade re-export lists
   (`cratestack-pg/src/lib.rs:93`, `cratestack-api/src/lib.rs:103`,
   `cratestack-sqlite/src/lib.rs:55`). Every write builder in both adapters
   takes a concrete `&'static ModelDescriptor<M, PK>` — all 15 files under
   `sqlx/src/query/write/` and `create`/`update`/`update_many`/`delete`/
   `delete_many`/`upsert` under `rusqlite/src/delegate/`. `ReadSource`, by
   contrast, is dispatched through at **twenty sites across fifteen files**
   in the two adapters, plus the `Send`-proof in
   `sql/src/descriptor/tests_view.rs:55`. So 14 of the descriptor SPI's 28
   methods (12 required + 2 defaulted on `ReadSource`, 14 on `WriteSource`)
   are a contract nobody dispatches through, exported on the public surface
   of all three facades.

4. **The runtime-registry form of a persistence SPI is already refused.**
   ADR 0012 rejects an IoC container permanently, and reason 2 is the
   binding one here: a registry needs its implementations reachable to
   register them, which makes `examples/no-database-verification`'s
   `cargo tree` proof of `sqlx`/`libsqlite3-sys` absence *unstateable*, not
   merely false. Any Store SPI that resolves a backend at runtime is
   already out of scope; only a compile-time-monomorphised one is even on
   the table.

5. **Three loose ends from #465 (`6f14f1e`) are still open**, and they are
   the cheapest available evidence about whether L1's boundary is
   understood:
   - `sql/src/idempotency.rs` (47 lines) opens
     `//! Idempotency DDL and utilities for Postgres.` inside a crate whose
     `lib.rs` opens `//! Dialect-agnostic SQL primitives`. The constant is
     `BYTEA` / `UUID` / `TIMESTAMPTZ` / `NOW()` — unrunnable on SQLite.
   - `cratestack-axum`'s **entire** dependency on `cratestack-sql` is one
     line: `idempotency/store.rs:4`,
     `pub use cratestack_sql::IDEMPOTENCY_TABLE_DDL;`. Nothing else in
     `cratestack-axum/src` mentions `cratestack_sql`.
   - `MAX_BODY_BYTES = 2 * 1024 * 1024` now sits at
     `core/src/store/idempotency.rs:12`, documented as the bound past which
     "a request beyond this returns 413". A 413 is an HTTP status. An L4
     concern is living in an L1 contract module — and `cratestack-axum`
     re-aliases it `pub(super)` at `idempotency/store.rs:8` anyway, so the
     L1 copy buys nothing.

6. **A stale doc understates what already exists.** `read_source.rs:8-12`
   still says the genericization to `&'static dyn ReadSource<M, PK>`
   "lands in a follow-up PR once the trait shape has settled". It landed;
   see the twenty sites in (3). Verified verbatim at `origin/main`.

## Decision

**CrateStack will freeze the Store SPI at the three operational traits —
`IdempotencyStore`, `RateLimitStore`, `AuditSink` — and will not grow a
storage-backend-swappable persistence SPI over models.** `cratestack-sql`
stays a query-planning vocabulary plus `Dialect`'s single placeholder
decision; per-model persistence stays monomorphised generated code against
a concrete descriptor and a concrete adapter, chosen by which facade the
application depends on.

The admission test for any *future* trait joining the frozen three is the
one `Dialect`'s doc already states, applied to storage: **two shipped,
independently-motivated implementations must already exist or be
committed** before the trait is introduced. `IdempotencyStore` (sqlx +
Redis) and `RateLimitStore` (in-memory + Redis) pass it. A
`Repository<M, PK>` does not: there is exactly one Postgres adapter and one
SQLite adapter, they are never selected against the same schema, and the
deliberate `row_pg.rs` (226 lines) / `row_sqlite.rs` (206 lines) fork
recorded in `layering.md` §2 (L2) says they should not be unified anyway.

The budget this frees goes to finishing what exists, as three mechanical
tickets. **This ADR records that these are the only cleanups its freeze
implies; it does not authorise the code movement.** Each would be worth
filing even if this ADR were rejected, and each needs its own PR:

1. Correct `sql/src/descriptor/read_source.rs:8-12` to describe the shipped
   state.
2. Resolve #465's three residues: move `IDEMPOTENCY_TABLE_DDL` to
   `cratestack-sqlx` (or rename it to admit it is Postgres-only), drop
   `cratestack-axum`'s now-empty `cratestack-sql` edge with it, and delete
   the L1 `MAX_BODY_BYTES` in favour of `cratestack-axum`'s own.
3. Decide `WriteSource`'s fate explicitly — give it a consumer, or record
   in its own doc comment that it is a type-level guard (views cannot
   implement it, so views cannot reach a write builder) rather than a
   dispatch seam. Today its doc implies the latter; its 14 methods imply
   the former.

**Out of scope, deliberately.** Whether `OpExecutor` gets built (ADR 0015),
and whether layer direction gets a CI check (ADR 0014), are separate
decisions. This one settles only the reach of the Store SPI.

## Consequences

### Positive

- The `dyn` seams stay countable. `layering.md` §4.2's discipline —
  fourteen `dyn ` occurrences in five files across `cratestack-axum/src` —
  survives, and so does the monomorphisation claim in CLAUDE.md's first
  paragraph.
- The `db = None` dependency-surface proof stays stateable. No shared
  storage trait means no registry means nothing to make
  `examples/no-database-verification` ambiguous.
- The facades stay 246 / 156 / 75 lines of re-export. A persistence SPI
  would have needed somewhere to be wired, and the facades are the only
  place that sees all three backends.
- The three finishing tickets are individually reviewable and carry no
  design risk, which is the opposite of the alternative's profile.

### Negative

- **A third database backend stays as expensive as `layering.md` §5.6 says
  it is, and this ADR chooses not to reduce that.** Whoever proposes one
  pays the 15%-shared / 85%-forked cost cold, including a Postgres-only
  `introspect/` they must either extend or exempt themselves from.
- **No answer to the Spring-Data expectation.** A user asking "can I keep
  the schema and swap the store" gets "recompile against the other facade",
  which only works for the two backends that exist and never mid-flight.
  That is a real product gap; this ADR accepts it rather than solving it.
- **`WriteSource` keeps costing something.** It is public on all three
  facades with no consumer. Pre-1.0 it is cheap to remove; freezing the SPI
  makes removal look like scope creep, so we will probably carry it, or pay
  a breaking change later that we could take for free today.
- **Moving `IDEMPOTENCY_TABLE_DDL` is itself breaking** for anyone
  importing it through a facade re-export. The mitigation is that
  #453/#454 (`32f89de`) shipped a breaking public-trait signature change
  the day before, so the bar is demonstrably low — but it is not zero, and
  the ticket must state the path change.
- **This forecloses nothing about SSE policy replay, and must not be sold
  as if it did.** Row-level `@@allow` stays compiled into SQL at L2, so
  `rpc-transport.md` §3.4a's gap — policy not replayed against
  outbox-sourced `ModelEvent<T>` — remains open. A persistence SPI would
  not have fixed it either; that is L3's job (`layering.md` §5.1). Freezing
  here neither helps nor hurts it.
- **The next operational concern that wants a store has a rule but no
  precedent.** The two-implementations test above is written down for the
  first time in this ADR and has never been applied prospectively.

### Deferred / what would reopen this

Any one of these is sufficient grounds to revisit:

1. A funded, concrete third database backend — at which point the honest
   question is not "widen L1" but "how much of the 85% is worth extracting
   *now that a second consumer exists*", which is exactly the shape
   `grpc-codegen-deduplication.md` Decision 3 argues for its own case.
2. Two adapters independently needing the same non-placeholder `Dialect`
   method. That is the trait's own stated trigger.
3. A demand to run generated model CRUD against a store that is not SQL —
   a document database, a key-value store, an HTTP-backed service. Nothing
   in `cratestack-sql` survives that, so it is a new design, not a widening.
4. `OpExecutor` landing (ADR 0015) and needing a backend-neutral read/write
   handle in order to evaluate row-level policy outside the query builders.
   This is the most likely trigger and the one to watch; it would make the
   case for a *read*-side seam long before a write-side one.
5. A second `WriteSource` implementation appearing for any reason — that
   alone converts alternative B below from "buys nothing" to "obviously
   correct".

## Alternatives considered

**A. Widen to a Spring-Data-shaped `Repository<M, PK>` / `Store<M, PK>`.**
The strongest case is real: it is the one seam that would let `OpExecutor`
call persistence without knowing the backend, which is the only mechanism
by which row-level policy could be replayed for SSE and WebSocket without
duplicating the policy compiler; it would give the deliberate `row_pg` /
`row_sqlite` fork a shared contract to be checked against; and it is
precisely the abstraction Spring Data earned its reputation on, so users
will ask for it by name. Rejected on three grounds. (i) *Size*: covering
today's surface means find_many / find_unique / aggregate / aggregate_count
/ aggregate_column / two projected reads, create / update / update_many /
delete / delete_many / upsert, plus transaction scoping and outbox drain —
roughly 15 write builders and 10 read builders in `cratestack-sqlx` alone,
every one of which becomes a method every backend must implement. That is
the failure mode `Dialect`'s doc names, at ten times the scale. (ii) *Cost
of dispatch*: as trait objects it puts `dyn` on the hot path of every CRUD
call, against §4.2's counted-indirection discipline; as generics the macro
emits the bound, which is what the concrete descriptor already achieves for
free. (iii) *No pull*: the write side has zero polymorphic consumers today
(see Context 3), so we would be generalising over one implementation. The
read side is genuinely closer — `ReadSource` already does much of it — and
if this is ever revisited, it should be revisited read-first.

**B. Half-widen: genericize the write path to `&'static dyn WriteSource<M,
PK>` for symmetry.** Strongest case: it finishes a job that is visibly
half-done, removes an asymmetry a newcomer will trip over, and is a
mechanical change confined to two crates. This is the closest call in this
ADR. Rejected because the reason `ReadSource` earns its trait object — two
impls, `ModelDescriptor` (`model_impls.rs:17`) and `ViewDescriptor`
(`view.rs:99`), dispatched from one builder — has no counterpart on the
write side and structurally cannot: views do not write, by the type-level
guarantee `read_source.rs:20-22` exists to provide. Genericizing over a
single permanent implementation is cost with no benefit. Revisit
immediately if a second `WriteSource` impl appears.

**C. Delete `WriteSource` outright.** Strongest case: it is unused, we are
pre-1.0, and deleting it makes the descriptor SPI's story honest — the
traits exist to separate model from view on the *read* path, full stop.
Rejected, narrowly, because the trait does one load-bearing thing besides
dispatch: its absence on `ViewDescriptor` is what makes routing a view
through `CreateRecord` / `UpdateRecord` / `DeleteRecord` a type error. That
guarantee is cited in `sqlx/src/delegate/view.rs:9`,
`rusqlite/src/delegate/view.rs`, `sql/src/descriptor/view.rs:5` and both
adapter READMEs. Finishing ticket 3 above should make that the documented
purpose rather than deleting the trait — but if review disagrees, deletion
is defensible and this ADR does not fight it.

**D. Freeze, and do nothing else.** Strongest case: the finishing work is
not architecture and does not belong in an ADR; bundling it makes the
decision look bigger than it is. Rejected because two of the three items
are direct evidence about the very boundary being frozen — a Postgres-only
constant in the dialect-agnostic crate, and a 413 bound in a contract
module, both landed *by the PR that was fixing layering* — and a freeze
that leaves them in place is a freeze at a boundary nobody can point to.
The compromise this ADR takes is to name them as tickets and explicitly
withhold authorisation for the moves.

## What the maintainer must decide

This ADR is **Proposed**, not Accepted. Four questions are open, in
descending order of consequence:

1. **Freeze, or leave the question open?** Accepting this closes the
   persistence-SPI direction until one of the five reopening triggers
   fires. *What would settle it:* whether a third backend or a non-SQL
   store is on any roadmap at all. If one is, alternative A's read-first
   variant deserves a spike before this is accepted.
2. **Is the two-implementations admission test the right bar?** It is
   proposed here for the first time and has never been applied
   prospectively. *What would settle it:* running it retrospectively
   against `AuditSink`, which fails twice over — `MulticastAuditSink` is
   not independently motivated from `NoopAuditSink`, and neither has a
   caller. If the test would have blocked a trait we shipped and still
   want, it needs rewording before it is adopted. **Update (post-#473):**
   the "neither has a caller" half of that premise no longer holds —
   `AuditSink::record` now has a real installation path
   (`SqlxRuntime::with_audit_sink`) and 11 call sites across
   `cratestack-sqlx`'s write paths — but `MulticastAuditSink` still is not
   independently motivated from `NoopAuditSink`, so the admission-test
   question stands and is the maintainer's to resolve, not re-decided here.
3. **`WriteSource`: document as a type-level guard, or delete?** *What
   would settle it:* whether anyone intends a second implementation. If
   not, it is a naming problem, not a design one.
4. **Does `IDEMPOTENCY_TABLE_DDL` move to `cratestack-sqlx`, or just get
   renamed in place?** Moving is cleaner and breaks a public path; renaming
   is free and leaves an L1 crate holding Postgres DDL. *What would settle
   it:* whether any consumer outside this repo imports it — currently
   unknown, and worth one search of published dependents before choosing.

Nothing in this ADR should be implemented before those four are answered,
except the `read_source.rs:8-12` doc correction, which is true regardless of
the outcome.
