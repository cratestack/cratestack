# ADR 0013: Facade and Macro Disjointness Is a Layer-Model Invariant

## Status

Accepted

## Date

2026-08-08

Context doc: [docs/design/layering.md](../design/layering.md)

## Context

CLAUDE.md states the rule and immediately admits how it is held up: "**Hard rule
(enforced by convention, watch for regressions):** the macro split must stay strictly
disjoint." Nothing checks it. `cargo deny check` — the only dependency gate in `just
all-checks` — has a `[bans]` section that is exactly `multiple-versions = "warn"` and
no concept of intra-workspace shape.

The rule is real today, and mechanically visible at `origin/main` (`08fbb7e`):

- The three facades are single-file re-export crates — `crates/cratestack-pg/src/lib.rs`
  is 246 lines, `cratestack-api/src/lib.rs` 156, `cratestack-sqlite/src/lib.rs` 75 —
  and their `[dependencies]` are disjoint **on the L2 database axis**. `cratestack-pg`
  has `cratestack-sqlx` (optional, default-on via `postgres`) and no
  `cratestack-rusqlite`; `cratestack-sqlite` has `cratestack-rusqlite` and neither
  `cratestack-sqlx` nor `cratestack-axum`; `cratestack-api` has neither adapter, under
  any feature. (`cratestack-sqlite` *does* carry `cratestack-client-rust`, under
  `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`, `Cargo.toml:63-64` — see
  the Decision's scoping note. That is an L4 binding, not a database adapter, and it is
  there deliberately.)
- No crate in the workspace links both database adapters. `cratestack-sqlx` appears in
  the `Cargo.toml`s of `cratestack-api`, `cratestack-macros`, `cratestack-cli` and
  `cratestack-pg` (in the first two only as feature-name comments; `cratestack-cli` is a
  tool, which the layer model exempts); `cratestack-rusqlite` appears in
  `cratestack-sqlite` and — as a comment only — `cratestack-cbor-wasm`.
- `guard_server_postgres_backend`
  (`crates/cratestack-macros/src/include/datasource_guard.rs:88`) turns `db = Postgres`
  under a sqlx-less facade into one `compile_error!`, by reading
  `cfg!(feature = "postgres")` against `cratestack-macros`' *own* compiled features —
  the forwarding mechanism `extensions.md` §2 documents after `CARGO_FEATURE_<NAME>`
  was empirically disproved in #161.
- `examples/no-database-verification` and `examples/no-database-verification-api` exist
  to hold a `cargo tree` proof that `sqlx`/`libsqlite3-sys` are absent, and live
  outside the workspace because Cargo's feature unification would otherwise mask it —
  spelled out in the root `Cargo.toml`'s `exclude` list.

The layer model puts this under new pressure, in three specific places. `layering.md`
§5.1 names L3 (execution) as missing, which invites a shared runtime middle. §5.6 notes
`cratestack-sql`'s `Dialect` trait has exactly one method, `write_placeholder`
(`dialect.rs:17`), and that query execution, transactions, audit, idempotency DDL,
migrations and introspection are all written per-backend — which invites "just abstract
the other 85%". `grpc-codegen-deduplication.md` Decision 3 recommends a new shared crate
on precisely the precedent "`cratestack-sql` shared by
`cratestack-sqlx`/`cratestack-rusqlite`" — a correct argument for *client* codegen that
generalises badly if applied to runtimes.

And §5.5 shows the pressure already produced a leak: `cratestack-sql/src/idempotency.rs`
(47 lines, added by #465, `6f14f1e`) opens `//! Idempotency DDL and utilities for
Postgres.` and holds a `BYTEA`/`TIMESTAMPTZ`/`UUID`/`NOW()` DDL constant that SQLite
cannot run, inside the crate whose `lib.rs` opens `//! Dialect-agnostic SQL primitives`.

## Decision

CrateStack will treat strict **backend** disjointness as an **invariant of the layer
model**, not a convention of `cratestack-macros`. Concretely:

`include_server_schema!(..., db = Postgres)` emits sqlx-only code.
`include_server_schema!(..., db = None)` emits axum-only code with no database machinery
at all. `include_embedded_schema!` emits rusqlite-only code. No cross-backend impl leaks
between the three paths, and **each consuming crate picks exactly one
database-owning entry macro.**

That last clause is deliberately narrower than "one entry macro per crate", because that
broader claim is false of the shipped code. `include_client_schema!` owns no backend —
it emits HTTP client stubs against another service's schema — and it legitimately
coexists with `include_embedded_schema!` in the same crate. Two examples do exactly
this: `examples/tauri-native/src-tauri/src/lib.rs:29,34` and
`examples/react-nextjs-daisyui/napi/src/lib.rs:38,43` each invoke both, over separate
schemas, in separate modules. It is the reason `cratestack-sqlite` carries a
target-gated `cratestack-client-rust` dependency at all. The invariant is about which
*database* a crate links, not how many macros it calls.

This **forbids**, permanently and without a feature-flag escape:

1. A shared runtime `Backend`/`Database` trait implemented by both `cratestack-sqlx` and
   `cratestack-rusqlite`.
2. A facade that depends on more than one L2 database adapter.
3. Any L1 addition that forces both adapters to link — a trait whose default body calls
   into one adapter, or an L1 type whose Cargo features pull one in.
4. Any L3 design that resolves a backend from a registry. When `OpExecutor` is built, it
   must be a function over an already-chosen set of collaborators, per ADR 0012.

It **permits**, and encourages, additive L1 contracts that each adapter implements or
consumes independently. `ReadSource<M, PK>` / `WriteSource<M, PK>`
(`crates/cratestack-sql/src/descriptor/read_source.rs`) are the existing correct shape:
defined at L1, implemented by `ModelDescriptor` (`model_impls.rs:17,62`) and — for
`ReadSource` only — `ViewDescriptor` (`view.rs:99`), consumed as
`&'static dyn ReadSource<M, PK>` by both adapters
(`cratestack-sqlx/src/query/read/find_many.rs:17`,
`cratestack-rusqlite/src/delegate/find_many.rs:16`), with neither adapter able to see the
other. The distinguishing test is what the trait abstracts: `ReadSource` abstracts
*descriptor shape* (model vs view), not *storage backend*. A trait object may cross the
L1→L2 boundary; the adapters may not cross each other.

The invariant stays checkable the way it is checked today: the `cargo tree` proofs of
absence in `examples/no-database-verification` and `examples/no-database-verification-api`.
Mechanising the check is deliberately **not** decided here — that belongs to ADR 0014,
which covers the same missing tooling for the whole graph.

## Consequences

### Positive

The `db = None` guarantee stays *stateable*, which is the property that
`examples/no-database-verification` exists to exercise and that `no-database-mode.md` §7
builds `cratestack-api` on: a service that never touches a database can prove it with
`cargo tree`, not with a runtime condition report. Monomorphisation stays intact —
`layering.md` §4.2's audit found `dyn ` in `cratestack-axum/src` exactly fourteen times
in five files. And a reviewer now has a rule they can apply to a new crate without
asking the maintainer: name the database adapter it links, and check nothing above it
links a second.

### Negative — what becomes harder

- A third database backend is not an L2 exercise. `layering.md` §5.6 is the honest
  accounting: `cratestack-sql` covers the query-planning vocabulary plus one abstracted
  decision, and `cratestack-migrate/src/introspect/` is 972 lines across twelve files,
  eleven of them under `introspect/postgres/` with no SQLite counterpart, so
  `cratestack migrate diff` against a live database is Postgres-only and a new backend
  inherits that. This ADR forbids the cheap way out.
- Deliberate duplication is now permanent, not tolerated.
  `macros/src/model/row_pg.rs` (226 lines) and `row_sqlite.rs` (206) stay two files
  forever, and a bug fixed in one must be fixed in the other by hand.
- `cratestack-sql/src/idempotency.rs` is reclassified from "small wart" to "defect".
  Under this ADR it must move to `cratestack-sqlx` or be renamed to admit it is
  Postgres-only; it can no longer be cited as precedent for putting the next
  dialect-specific artefact in `cratestack-sql`. (The move itself is ADR 0016's ticket 2,
  not authorised here.)
- **The enforcement is weaker than it looks, and this ADR does not fix that.**
  `grep -rn "no-database" .github/ justfile` returns nothing. The `cargo tree` proof is a
  README instruction a human runs, not a CI job; the examples' own test suites
  (`examples/no-database-verification-api/tests/smoke.rs`) prove the generated router
  works, not that `sqlx` is absent. `libsqlite3-sys`'s `links = "sqlite3"` collision —
  the mechanical reason cited in `cratestack-pg/src/lib.rs:7-11` — self-enforces
  forbidden item 2 for the sqlx+rusqlite pair only; a hypothetical `cratestack-api` +
  rusqlite facade would compile happily. Accepting this ADR buys vocabulary and a
  reviewable rule, not a gate.

### Foreclosed options

Runtime backend selection (one binary, backend chosen by config). A single facade
covering server and embedded *database* roles for offline-first apps that want both —
such an app takes two crates, one per role, as `examples/embedded-*` already do. Any
future `OpExecutor` that dispatches on a backend registry.

### What would make us revisit

A concrete consumer needing one process to hold a Postgres server role *and* an embedded
SQLite role in the same crate, where two crates is demonstrably not workable. Or
`cratestack-sql` growing — through honest, independently motivated additions — past the
point where a second dialect is mostly-free, at which point the cost side of alternative
A below changes and it deserves a re-run.

## Alternatives considered

**A. A shared runtime backend trait (`Backend` / `Database`) implemented by both
adapters.** Its strongest case is real and should not be waved away: it would collapse
`include/server.rs` (239 lines) and `include/embedded.rs` (238) toward one composer,
retire the two-vocabulary asymmetry `layering.md` §5.2 describes (`ServerDb` for
server-side, a separate top-level composer for embedded), make a third backend one impl
rather than a fork, and it has a genuine in-repo precedent — `cratestack-migrate`'s
`ir/` + `emit/{postgres,sqlite}/` split, which `layering.md` §4.4 calls the cleanest
per-dialect split in the repo. Rejected because a registry needs its implementations
*reachable* to register them, and reachability is exactly what
`examples/no-database-verification` proves the absence of; the `db = None` guarantee
would not become false, it would become unstateable. Secondarily, §5.6's accounting says
the migrate precedent does not transfer: `migrate`'s `Op` IR is a pure data structure
with no connection, transaction, or row decoding in it.

**B. A fourth facade depending on both database adapters, for offline-first apps.**
Strongest case: a real shape exists — a server that also maintains a local SQLite cache
— and today it costs two crates and a workspace boundary. Rejected on the
`libsqlite3-sys` `links = "sqlite3"` ground: the graph is hostile, and
`cratestack-pg/src/lib.rs:7-11` documents that this is precisely why the two facades are
kept apart. (An earlier draft of this ADR also rejected it on the ground that "a
consuming crate can only invoke one entry macro anyway." That ground is false —
`examples/tauri-native/src-tauri` invokes two — and has been withdrawn. The
`links`-collision argument stands on its own.)

**C. Leave the rule where it is, as a CLAUDE.md convention.** This is the closest call,
and the decision is marginal on one axis: writing it down adds no enforcement, and the
convention has in fact held. What tips it is that the convention is stated as a fact
about `cratestack-macros`, so it does not answer an argument pitched at
`cratestack-sql` — and `grpc-codegen-deduplication.md` Decision 3 has now made exactly
that kind of argument, correctly, for client codegen. The convention text has no reply to
"we already share an L1 crate between the two adapters, so why not share more". This ADR
supplies the reply, and names the test (backend vs. descriptor shape) that separates the
two cases. It also corrects the convention's own shorthand — "pick one per consuming
crate" — which two shipped examples contradict.

**D. Enforce direction and disjointness in CI.** Not rejected — out of scope. It is one
decision per ADR, and this is not that decision; the same missing tooling covers the
whole dependency rule, so it belongs to ADR 0014, not here.
