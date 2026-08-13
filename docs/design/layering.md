# Layering — names for the strata CrateStack already has

Status: **descriptive + one proposal.** Sections 1–5 name what is already
true at `origin/main` (`08fbb7e`) and are meant to be adopted as
vocabulary, not implemented. Section 6 argues a permanent refusal.
Section 7 hands the genuinely open calls to ADRs rather than settling
them here.
Scope: the whole workspace's crate graph. No `.cstack` grammar change, no
runtime behaviour change, no crate is proposed for deletion or splitting.
Tracking: this document is the source of truth for the ADRs listed in §8.

> **This is not a refactor proposal.** Five of the six layers below
> already exist as real crates with real trait boundaries. The one that
> does not (L3) has been specified for a year in
> [`rpc-transport.md`](rpc-transport.md) §4 and deliberately gated in §6.5.
> If you read a sentence here as "move a lot of code", you have read it
> wrong.

> **Merge-order note.** Everything below is measured against `origin/main`
> at `08fbb7e`. That commit includes #464 (`cfde4e0`), #465 (`6f14f1e`) and
> #459, and the two design documents this paper cites most heavily —
> [`grpc-codegen-deduplication.md`](grpc-codegen-deduplication.md) and
> [`trusted-proxy-client-ip.md`](trusted-proxy-client-ip.md) — landed in
> `0da6e00`. A branch cut before those will not contain them, and on such a
> branch `cratestack-redis` still lists `cratestack-axum` under
> `[dependencies]` — i.e. §1's back-edge is still live. Rebase onto
> `origin/main` before reading the verification claims as current.

> **Post-#475 update.** §3's "two upward edges remain" and §9's item 4 are
> now stale: #475 moved `ClientStateStore` (and its companion types) from
> `cratestack-client-rust/src/state.rs` down into
> `cratestack-core::store::client_state`, the same fix #465 applied
> server-side, applied to the client-side twin this document itself named
> as the open instance. `cratestack-client-store-redis` no longer depends
> on `cratestack-client-rust` at all; `cratestack-client-store-sqlite` keeps
> it only as a `[dev-dependencies]` entry (test fixtures), which is the same
> shape §3 already accepts for `cratestack-redis -> cratestack-axum`.
> Verified: `cargo tree -p cratestack-client-store-sqlite -i
> cratestack-client-rust` reports a dev-dependency path only;
> `cargo tree -p cratestack-client-store-redis -i cratestack-client-rust`
> reports no path. The two `L2 -> L4 !` rows in §3's table, the paragraph
> beneath it, §7's "two open client-side back-edges" bullet, and §9 items
> 4/2 (Reviewer notes) describe the pre-#475 state and are left as the
> historical record the "as of #465 there are no back-edges" miss already
> established this document keeps rather than silently editing away — read
> them as **fixed by #475**, not as current.

## 1. Why this document exists

The architecture works. It does not explain itself.

CLAUDE.md states the dependency flow in one line — "parser →
core/policy/sql → macros → backend runtimes / clients" — and that line is
load-bearing: PR #465 (`6f14f1e`, "fix(cratestack-core,cratestack-sql):
move storage traits and DDL from cratestack-axum to core/sql") cites it
verbatim as the rule that had been violated. The violation was real and
unglamorous: `cratestack-sqlx` and `cratestack-redis` both depended on
`cratestack-axum` — an HTTP transport crate — for no reason other than
that `IdempotencyStore` and `RateLimitStore` happened to have been
*defined* there. Two storage adapters were downstream of the web layer.
#465 moved the traits into `cratestack-core::store::{idempotency,ratelimit}`
and `IDEMPOTENCY_TABLE_DDL` into `cratestack-sql::idempotency`, and the
commit message records the verification: `cargo tree -p cratestack-sqlx -i
cratestack-axum` → no match.

Nothing caught that for however long it existed. Nothing would catch the
next one — and, as §3 now records, nothing did: the same defect exists
today on the client half of the graph. `cargo deny check` — the only
dependency gate in `just all-checks` (`justfile:37-43`) — validates
licences, advisories, bans and sources; its `[bans]` section in `deny.toml`
is exactly `multiple-versions = "warn"`. It has no concept of
intra-workspace direction and never will; that is not what it is for.

But the deeper problem is not the missing check. It is that the rule was
stated as a chain of five crate names, and the workspace now has thirty
members across thirty-one crate directories.
"parser → core/policy/sql → macros → runtimes" does not tell you where
`cratestack-grpc` goes, or `cratestack-proto`, or the client-state stores,
or whether `cratestack-axum` may depend on `cratestack-sql` (it does, as
of #465 — see §5.5). A rule you cannot apply to a new crate without asking
the maintainer is not an architecture; it is a habit.

So: names. Six numbered layers plus one orthogonal thing. The point is to
make true statements *sayable* — "audit fan-out is an L3 concern declared
at L1 with no caller anywhere" is a sentence that could not previously be
uttered, and once it can be, the question of whether that is acceptable
becomes askable. Several such sentences appear in §5 and they are not
flattering.

## 2. The layer model

| Layer | Name | Crates | src LoC |
|---|---|---|---|
| **L0** | Schema IR | `cratestack-parser`, `cratestack-core::schema` | 9,265 / — |
| **L1** | Contracts | `cratestack-sql`, `cratestack-core::{store,audit,codec}`, `cratestack-policy` | 2,799 / — / 822 |
| **L2** | Adapters | `cratestack-sqlx`, `cratestack-rusqlite`, `cratestack-redis`, `cratestack-codec-{cbor,json}`, `cratestack-client-store-{sqlite,redis}` | 10,907 / 4,828 / 2,167 / 59 / 37 / 305 / 245 |
| **L3** | Execution | *(nothing — see §5.1)* | 0 |
| **L4** | Bindings | `cratestack-axum`, `cratestack-grpc`, `cratestack-client-{rust,dart,typescript,flutter}` | 5,212 / 752 / 3,269 / 4,537 / 3,525 / 553 |
| **L5** | Facades | `cratestack-pg`, `cratestack-api`, `cratestack-sqlite` | 246 / 156 / 75 |
| **⊥** | Compiler | `cratestack-macros` (+ `cratestack-proto`) | 18,172 / 3,013 |

(LoC counted over each crate's `src/` including its in-crate `tests_*.rs`
files, which is where this workspace keeps most unit tests.)

This table is illustrative, not exhaustive: it names the crates the prose
below argues about. **The complete, every-crate assignment — including
`cratestack-cbor-{napi,wasm}`, `cratestack-mock-wiremock`, the tools, and
the empty `cratestack` vitrine crate — is settled in ADR 0011, not here.**
A crate that appears in neither this table nor that one is unclassified,
which is itself a defect.

### L0 — Schema IR

**Belongs here:** the `.cstack` grammar, the semantic checker, and the
in-memory representation of a parsed schema. `cratestack-parser` (chumsky
+ a hand-rolled top-level dispatch, `parse/mod.rs`) produces
`cratestack_core::schema::{Schema, Model, Field, View, Procedure, ...}`.
`cratestack-core` has zero workspace dependencies; `cratestack-parser`
depends on it and on nothing else in the workspace. That is the bottom of
the graph and it is genuinely clean.

**Must not be here:** anything that knows a backend exists. L0 knows
`provider = "postgresql"` is a *string in a datasource block*; it does not
know what Postgres is. The one place L0 gets an opinion about a backend is
`extension pgvector { }` being rejected outright under
`include_embedded_schema!` — and even that rejection lives at ⊥
(`guard_embedded_declared_extensions`,
`macros/src/include/extension_gate.rs:137`), not in the parser.

### L1 — Contracts

**Belongs here:** trait and data definitions that two or more layers above
must agree on, with no implementation that binds a backend.

- `cratestack-sql` — the SQL vocabulary shared by both database adapters:
  `Filter`/`FilterExpr`/`OrderClause` (`src/filter/`), `SqlValue`/
  `IntoSqlValue` (`src/values/`), `ModelDescriptor`/`ViewDescriptor`, the
  `ReadSource`/`WriteSource` traits (`src/descriptor/read_source.rs`), and
  the `Dialect` trait (`src/dialect.rs`).
- `cratestack-core::store` — `IdempotencyStore`, `RateLimitStore`,
  `RateLimitConfig`, `RateLimitDecision` (post-#465).
- `cratestack-core::audit` — `AuditSink` (`src/audit.rs:81`) plus its two
  in-tree implementations, `NoopAuditSink` and `MulticastAuditSink`. See
  §4.2 for what this seam does and does not currently do.
- `cratestack-policy` — policy literals, predicates, and
  `authorize_procedure` (`src/eval.rs:9`), which is pure: it evaluates a
  procedure policy against an auth context with no database in sight.

**Must not be here:** SQL text that only one dialect can execute, or a
bound that belongs to a wire protocol. L1 owns the *shape* of a query, not
its rendering. This rule is currently broken in two places — see §5.5.

### L2 — Adapters

**Belongs here:** everything that knows how one specific external system
actually behaves. `cratestack-sqlx` renders and executes Postgres SQL,
owns transactions, decodes `PgRow`, and drains the event outbox.
`cratestack-rusqlite` does the same against SQLite, on native *and*
`wasm32-unknown-unknown`. `cratestack-redis` implements the two operational
store traits against Redis.

The wire codecs sit here for the same reason: `CoolCodec` is a trait in
`cratestack-core::codec` (L1), and `cratestack-codec-cbor` /
`cratestack-codec-json` are two implementations of it (L2). The client
state stores (`cratestack-client-store-sqlite`, `-store-redis`) are the
client-side instance of the same pattern — and, as §3 records, the only
crates in the workspace that currently point the wrong way.

**Must not be here:** anything a second adapter would have to reimplement
identically, and anything that decides *policy* rather than *mechanism*.
An adapter answers "how do I write this row"; it must not answer "may this
caller write this row" or "has this request already been served". Row-level
policy answers the second of those from inside L2 — see §5.1.

**The two adapters are a deliberate fork, not duplication.**
`macros/src/model/row_pg.rs` (226 lines) and `row_sqlite.rs` (206 lines)
are near-mirrors, and that is correct: a row codec is exactly the kind of
thing that should be written twice rather than abstracted once. The layer
model does not ask for these to be unified.

### L3 — Execution

**Would belong here:** the transport-neutral middle of an operation —
policy enforcement, idempotency reservation and replay, rate-limit
admission, audit *fan-out*, event publication — expressed once, against
`(op_id, principal, idempotency key, request bytes)`, with no knowledge of
whether the caller arrived over REST, RPC, gRPC, SSE, or a future
WebSocket.

**Must not be here:** anything transport-shaped (`http::HeaderMap`,
`tower::Layer`, `axum::Response`) and anything backend-shaped
(`sqlx::Transaction`). That second exclusion is load-bearing and it bites:
audit *persistence* cannot move here, because
`cratestack-sqlx/src/audit.rs:1-5` guarantees the audit row commits inside
the mutation's own transaction. Only fan-out is L3-shaped.

This layer does not exist. `rpc-transport.md` §4 already names it —
"idempotency, ratelimit, and audit cannot remain HTTP-only `tower::Layer`s.
They move into a small `OpExecutor` service in `cratestack-core` (or a new
crate)" — and §6.5 gates building it on a concrete WebSocket or
multiplexing case that has not appeared. `grep -rn OpExecutor` over the
repo returns hits in three design documents and the CHANGELOG, and zero
lines of code. §5.1 describes what its absence costs today.

### L4 — Bindings

**Belongs here:** one wire protocol's worth of encode/decode/route/status
mapping. `cratestack-axum` owns REST routes, the RPC dispatcher, cbor-seq
and SSE encoders, header extraction, and the two `tower::Layer`s.
`cratestack-grpc` owns the tonic surface. The four client-generator crates
own the outbound half in four languages.

**Must not be here:** a rule about *whether* an operation may run. A
binding decides how a decision is expressed on the wire (which status
code, which `ErrorBody.code`); it must not be the only place the decision
is made. This is currently broken — see §5.1.

### L5 — Facades

**Belongs here:** a `[lib] name = "cratestack"` rename, a set of
re-exports, and a Cargo feature graph. Nothing else. And that really is
all they are: `cratestack-pg` is 246 lines of `src/`, `cratestack-api`
156, `cratestack-sqlite` 75 — most of which is doc comment.

**Must not be here:** logic. A facade that grows a function has stopped
being a facade.

### ⊥ — The compiler

`cratestack-macros` (18,172 lines) is not a layer. It is a compiler that
runs at `rustc` time and *emits into* L2 through L5: `model/` and `view/`
emit descriptors and row codecs that L2 consumes, `axum/` and `transport/`
emit handlers that live at L4, `client/` emits the generated Rust client,
and `include/{server,embedded,client}.rs` compose the whole module. It
depends downward on `cratestack-{core,parser,policy,proto}` and on nothing
above L1 — correct, since a compiler must not link its own output.

That "nothing above L1" is a real constraint that a plain integer
comparison cannot express, because L5 depends on ⊥ and ⊥ must not depend on
L2. ⊥ needs a *rule*, not a number; ADR 0014 records this as an open
question rather than inventing an answer.

`cratestack-proto` sits alongside it for the same reason (lockfile
ownership + `.proto` text emission). `cratestack-cli`, `cratestack-lsp`,
`cratestack-migrate` and `cratestack-studio` are tools, not layers; they
consume L0/L1 and are consumed only by each other.

**The compiler is parameterised upward by L5.** `cratestack-pg`'s
`postgres`, `grpc`, `rate_limit` and `pgvector` features each forward to a
same-named feature on `cratestack-macros`, which the macro reads via
`cfg!(feature = "...")` against *its own* compiled feature set — the
mechanism `extensions.md` §2 documents at length after the
`CARGO_FEATURE_<NAME>` approach was empirically disproved in #161. This is
information flowing L5 → ⊥ without a dependency edge in that direction,
and the layer model has to accommodate it rather than pretend it is a
violation. It is how `guard_server_postgres_backend`
(`macros/src/include/datasource_guard.rs:88`) can say "this facade has no
sqlx" at expansion time.

## 3. The dependency rule

> **A crate at layer N may depend on crates at layers ≤ N, and on nothing
> at a layer > N. Same-layer edges are legal provided the intra-layer graph
> stays acyclic.** `cratestack-macros` may depend on L0 and L1 only. Tools
> may depend on anything; nothing at L0–L5 or ⊥ may depend on a tool.

An earlier draft of this section wrote `< N` — strictly downward, no
same-layer edges. That form is false of the graph it was written to
describe: `cratestack-sql -> cratestack-policy` (both L1) and
`cratestack-client-flutter -> cratestack-client-rust` (both L4) are legal
edges with no defect behind them. `≤ N` is the minimum revision that keeps
the rule true while still catching every defect the strict form catches;
both the fixed `sqlx -> axum` edge and the open `client-store-* ->
client-rust` edges are *upward*, and remain violations either way. ADR 0011
carries the argument.

Verified against `origin/main` (`08fbb7e`), workspace `[dependencies]` only:

```
cratestack-core         ->  (none)
cratestack-parser       ->  core
cratestack-policy       ->  core
cratestack-sql          ->  core, policy                     (L1 -> L1)
cratestack-macros       ->  core, parser, policy, proto
cratestack-proto        ->  core
cratestack-sqlx         ->  core, policy, sql
cratestack-rusqlite     ->  core, sql
cratestack-redis        ->  core
cratestack-axum         ->  core, sql
cratestack-grpc         ->  core
cratestack-client-rust  ->  core, codec-cbor, codec-json
cratestack-client-dart  ->  core, proto
cratestack-client-ts    ->  core, proto
cratestack-client-flutter -> client-rust                     (L4 -> L4)
cratestack-client-store-sqlite -> client-rust                (L2 -> L4 !)
cratestack-client-store-redis  -> client-rust                (L2 -> L4 !)
cratestack-pg           ->  core, parser, policy, sql, macros, sqlx, axum, grpc, client-rust
cratestack-api          ->  core, parser, policy, sql, macros, axum, client-rust
cratestack-sqlite       ->  core, parser, policy, sql, macros, rusqlite,
                            client-rust  [target: cfg(not(target_arch = "wasm32"))]
```

**Two upward edges remain, and they are the client-side twin of the defect
#465 fixed.** `pub trait ClientStateStore` is defined at
`crates/cratestack-client-rust/src/state.rs:43` — a binding — and
`cratestack-client-store-sqlite` and `cratestack-client-store-redis` depend
on that binding crate for the sole purpose of implementing it. Same shape,
same cause (the trait was defined where it was first used), unfixed. An
earlier draft of this section asserted "as of #465 there are no
back-edges"; that was true of the fourteen crates it happened to enumerate
and false of the graph. Enumerating all thirty found these on the first
pass, which is the honest argument for §8's enforcement ADR and is recorded
as such rather than quietly repaired.

Two other things worth reading off the table. `cratestack-sqlite`'s
`cratestack-client-rust` edge is a **target-gated** normal dependency
(`Cargo.toml:63-64`, `cfg(not(target_arch = "wasm32"))`) added for hybrid
consumers that ship an embedded DB *and* call a remote service; it is L5 →
L4 and legal, but any mechanical checker has to model `target` as well as
`kind`. And no facade depends on `cratestack-redis` — an application that
wants Redis-backed idempotency adds that dependency itself and passes an
`Arc<dyn IdempotencyStore>`. That is the layer model working: an adapter is
chosen by the application, not by the facade.

**Nothing enforces this.** The rule held on the server side because a human
ran `cargo tree -i` and opened #465. It did not hold on the client side.
`cargo deny` cannot express it. Whether to mechanise the check is a real
cost/benefit call and is handed to an ADR (§8), not decided here — but the
paper should be honest that the current enforcement mechanism is "someone
notices", and that it has already missed once.

## 4. Where the model is already true

Being generous here is not politeness; these are the parts a newcomer
should be told to copy.

### 4.1 The facades are a better starter mechanism than Spring Boot's

A Spring Boot starter is a POM plus auto-configuration classes discovered
at runtime through `META-INF/spring/…AutoConfiguration.imports`, applied
or skipped by `@ConditionalOnClass`/`@ConditionalOnMissingBean`. The
question "what am I actually getting" is answered by reading conditional
annotations, and in the hard cases by running the application and reading
the condition-evaluation report.

A CrateStack facade answers it with `cargo tree`. `cratestack-api`'s
`Cargo.toml` has no `cratestack-sqlx` entry at all — not optional, not
feature-gated — so "this service cannot reach a database" is a property of
the dependency graph, not of a runtime condition
(`no-database-mode.md` §7). Picking the wrong facade produces a single
`compile_error!` from `guard_server_postgres_backend`, not a
`NoSuchBeanDefinitionException` at startup. And the guarantee is *proven*:
`examples/no-database-verification` and
`examples/no-database-verification-api` exist primarily to hold a `cargo
tree` proof of absence (each also carries a `tests/smoke.rs` that
round-trips a real HTTP call, so the proof is not merely a `Cargo.toml`
assertion), and live outside the workspace because Cargo's feature
unification would otherwise mask the gate — the root `Cargo.toml`'s
`exclude` list says so in eight lines of comment.

This is the strongest part of the architecture and it should be named as
such. It is also the part most easily destroyed — see §6.

### 4.2 The store traits are runtime DI, at two working seams and one declared one

`Arc<dyn IdempotencyStore>` (`axum/src/idempotency/layer.rs:18`) and
`Arc<dyn RateLimitStore>` (`axum/src/ratelimit/layer.rs:16`) are real
substitution points with real alternatives: `SqlxIdempotencyStore`
(`sqlx/src/idempotency.rs:58`) vs `RedisIdempotencyStore`
(`redis/src/idempotency/trait_impl.rs:15`); `InMemoryRateLimitStore`
(`axum/src/ratelimit/store.rs:35`) vs `RedisRateLimitStore`
(`redis/src/ratelimit/trait_impl.rs:14`). `extensions.md` §2 already
generalises this as layer 3 of the extension mechanism — "the framework
ships one reference implementation per extension; using it is the default,
not the only option".

**`AuditSink` is the third seam only on paper, and the difference matters.**
`git grep AuditSink -- crates/ examples/` at `08fbb7e` finds the trait
(`core/src/audit.rs:81`), its two in-tree impls, one README line and one
doc cross-reference. The only call to `record()` is `MulticastAuditSink`
fanning out to its own children (`audit.rs:117`). `cratestack-macros`
contains zero mentions of it, and the generated `CratestackBuilder`
(`macros/src/include/server/runtime/postgres.rs:46-48`) has exactly one
field, `SqlxRuntime` — there is no builder method that accepts a sink.
`AuditSink` is a declared seam awaiting a consumer, not a working one.
Saying "three DI seams" flatters the design by one; the correct count is
two working plus one reserved, and ADR 0015 turns that into an argument.

Two corrections while we are counting. `cratestack-sqlx` implements
`IdempotencyStore`, **not** `RateLimitStore` — there are exactly two
production `RateLimitStore` impls, in-memory and Redis. `extensions.md`
§2 layer 3 and §5 both say "three interchangeable backends"; that phrasing
is wrong and should be corrected in that document, not just noted here.

The discipline around the seams is the interesting part. Across all of
`cratestack-axum/src`, excluding `tests*` files, `dyn ` appears exactly
fourteen times in five files: the two store traits, two
`Arc<dyn Fn(&Request) -> String>` key/fingerprint hooks, the boxed futures
`tower::Service` forces on you, and one
`Box<dyn erased_serde::Serialize>` in `projection.rs:57`. Everything else
— every handler, every route, every delegate — is monomorphised generated
code. The framework did not sprinkle indirection; it spent it deliberately
and countably.

### 4.3 `ReadSource` is a real SPI, and further along than its own doc says

`ReadSource<M, PK>` (12 required methods, 2 defaulted) and its supertrait
`WriteSource<M, PK>` (14 more) in
`sql/src/descriptor/read_source.rs` are implemented by `ModelDescriptor`
(`model_impls.rs:17`, `:62`) and — for `ReadSource` only — `ViewDescriptor`
(`view.rs:99`), and consumed by *both* adapters:
`sqlx/src/query/read/find_many.rs:17` and
`rusqlite/src/delegate/find_many.rs:16` both hold
`&'static dyn ReadSource<M, PK>`. The `Send + Sync` bounds are there for a
concrete reason, documented in the trait: an Axum handler future captures
the trait object across an `.await`, and without them the future stops
being `Send`.

The read-only-ness of views is enforced *at the type level* rather than by
a runtime check — `ViewDescriptor` does not implement `WriteSource`
(`read_source.rs:20-22`), so `CreateRecord`/`UpdateRecord`/`DeleteRecord`
cannot accept one. That is a better guarantee than any container-based
framework can offer.

**Correction worth recording:** the module doc at
`read_source.rs:8-12` still says "the existing builders still take
`&'static ModelDescriptor<M, PK>` today; the genericization to
`&'static dyn ReadSource<M, PK>` … lands in a follow-up PR once the trait
shape has settled." That follow-up has landed. `&dyn ReadSource` appears at
twenty sites across fifteen files in the two adapters —
`find_many.rs:17`, `find_unique.rs:14`, `aggregate.rs:35`,
`aggregate_column.rs:17,26`, `aggregate_count.rs:12,19`,
`projected_find_many.rs:16`, `projected_find_unique.rs:17`,
`support/conditions.rs:37,92`, `render/select.rs:16` in `cratestack-sqlx`,
and the five `rusqlite/src/delegate/*` sites plus
`rusqlite/src/render/select.rs:16,87,126`. The doc is stale and should be
corrected — it is currently understating how much of L1 is real.

### 4.4 `cratestack-migrate`'s emit split is the reference L1/L2 shape

`migrate/src/ir/` defines a dialect-agnostic `Op` IR; `emit/postgres/` and
`emit/sqlite/` each render it. Neither emitter knows the other exists;
adding a third means adding a directory, not editing a `match`. This is
the cleanest per-dialect split in the repo and the shape any future
backend work should copy — with the caveat §5.6 records, that `migrate`'s
`Op` IR is a pure data structure with no connection, transaction or row
decoding in it, which is why the precedent does not transfer to runtimes.

### 4.5 The `runtime` dispatch already does the right thing with `db`

`macros/src/include/server/runtime.rs` is 35 lines whose entire job is to
choose between `runtime/postgres.rs` (113 lines) and `runtime/none.rs`
(82 lines). It does not branch inline; it dispatches to a module. When
§5.2 complains about `ServerDb` branching, this file is the counter-example
that shows what the fix looks like.

## 5. Where the model is not yet true

### 5.1 L3 is missing, and the symptom is precise

The interesting claim is not "there is no `OpExecutor`". It is that the
four concerns L3 would own are currently distributed across *three
different layers*, inconsistently:

| Concern | Contract | Implementation | Applied at |
|---|---|---|---|
| Policy (procedure) | L1 `policy/src/eval.rs` | L1 (pure) | ⊥-generated `authorize_with_db` (`macros/src/procedure/instrument.rs:44`) |
| Policy (row-level) | L1 `ReadPolicy` literals | **L2** — compiled into SQL, `sqlx/src/query/support/policy.rs` | inside the query |
| Idempotency | L1 `core::store::idempotency` | L2 sqlx / redis | **L4** `tower::Layer` |
| Rate limit | L1 `core::store::ratelimit` | L2 redis / L4 in-memory | **L4** `tower::Layer` |
| Audit (persistence) | — | L2 `sqlx/src/audit.rs` | L2, inside the mutation's transaction |
| Audit (fan-out) | L1 `core::audit::AuditSink` | L1 (`MulticastAuditSink`) | **nowhere — no caller** |

Read the "Applied at" column. Idempotency and rate limiting fire from the
binding; row-level policy fires from inside generated SQL; audit fan-out
fires from nowhere at all. There is no layer at which you can stand and see
an operation whole.

One row of that table is *not* a misplacement, and the distinction is worth
protecting. Audit **persistence** is at L2 because it must be:
`sqlx/src/audit.rs:1-5` states the invariant — "Audit rows write inside the
mutation's transaction — you can never see a committed row whose audit
entry didn't also commit" — and `enqueue_audit_event` is called with
`&mut *tx` from every writer under `query/write/*` and `query/batch/*`. Any
L3 that owned audit persistence would have to thread a
backend-specific `&mut Transaction` through a transport-neutral interface,
which L3's own exclusion forbids, or write the row outside the transaction
and silently downgrade the guarantee. Only fan-out is L3-shaped, and
fan-out has no caller (§4.2). "Move audit to L3" is therefore either a
correctness regression or the relocation of dead code.

That is what "L3 is missing" actually means, and it is already producing
consequences other documents have had to work around:

- `rpc-transport.md` §3.4a records that row-level `@@allow` is **not**
  replayed against streamed `ModelEvent<T>` items, "that machinery lives in
  the SQL query builders and has no analogue for an in-memory
  outbox-sourced event" (restated as a scope limit in §6.5). That is not a
  policy gap; it is a layering gap. Policy lives at L2, and SSE does not go
  through L2.
- `trusted-proxy-client-ip.md` decision 6 records that `transport grpc`
  builds a *second* `axum::Router` via `into_router()`
  (`macros/src/include/server/grpc/service.rs:187`), so a consumer who
  layers protection onto `router()`'s output leaves gRPC exactly as
  exposed as before. Cross-cutting concerns applied as `tower::Layer`s
  attach to router instances, and there are now two. Commit `08fbb7e`
  (#416/#459) is the concrete instance: a spoofable-header fix applied
  inside `IdempotencyLayer` and `RateLimitLayer` — once per `Layer`, and
  therefore only on the routers the consumer remembered to layer.
- `idempotency-rate-limit-declarative-surface.md` §4.2 has had
  `@no_idempotency` codegen deferred *since it was written*, explicitly
  "gated on `OpExecutor`" — the ticket sketch in its §6 says so in its
  title, and adds "this should not be opened until `OpExecutor` has a
  concrete plan". A feature has been blocked for two release cycles on a
  layer that does not exist.

None of this argues for building L3 today. It argues that the cost of not
having it is now itemisable, which is what §6.5's gate needs in order to
be re-evaluated honestly. That re-evaluation is ADR work (§8).

### 5.2 The backend axis is branched at two different granularities

`ServerDb` (`macros/src/include/parse.rs:24`) is threaded as an explicit
parameter through **eight** function signatures across six non-test files,
and `ServerDb::{Postgres,None}` is mentioned 31 times across nine files,
seven of them non-test. That is a much smaller number than folklore
suggests and the files are well-chosen: `parse.rs` defines it,
`datasource_guard.rs` validates it against the schema's own `datasource`
block, `runtime.rs` dispatches on it to submodules (§4.5), and only
`axum_module.rs`, `axum_module/{model_router,router_fn}.rs` and `server.rs`
`match` on it to shape emitted tokens.

The problem is not mess. It is that the *embedded* backend is not a
`ServerDb` variant at all — it is a separate top-level composer,
`include/embedded.rs`. So "which backend" is asked in two different
vocabularies at two different granularities, and a hypothetical third
server-side backend would have to answer eight `ServerDb`-shaped questions
while a third *embedded-class* backend would fork a 238-line composer.
Naming L2 does not fix this. It does make it possible to say that the
asymmetry is in ⊥, not in the layer model, and that neither shape is
wrong — they are answers to different questions that currently look like
the same question.

*(Correction: an earlier informal count of "39 scattered `match db` sites,
worst offender `datasource_guard.rs` with 33" circulated during research
for this document. Both figures are wrong. The real count is 31 across
nine files, seven of them non-test, and `datasource_guard.rs` has six —
it is a single-purpose validation file, the opposite of a scattered
branch site.)*

### 5.3 The three composers triplicate their scaffold

`include/server.rs` (239 lines), `include/embedded.rs` (238) and
`include/client.rs` (190) each independently `quote!` a
`pub mod cratestack_schema { … }` shell: `SCHEMA_PATH`, `SCHEMA_SOURCE`,
`SCHEMA_SHA256` (with a near-identical eight-line doc comment in all
three), the `MODELS`/`TYPES`/`ENUMS` const arrays and their `*_COUNT`
siblings, then `pub mod types` / `pub use types::*` / `pub mod models` /
`pub use models::*` / field modules / `pub mod inputs` /
`pub use inputs::*`.

The deltas are real and load-bearing — server emits `VIEWS`,
`TRANSPORT_STYLE`, an axum module and a runtime block; embedded emits no
`PROCEDURES` and no `use ::cratestack::serde;` inside `types`/`models`;
client emits no `MIXINS` and no descriptors — so this is roughly 80% shared
with a genuinely varying 20%, not copy-paste. But it means the *shape of
the generated surface* has three owners, and a change to it (adding a
const, renaming a submodule) is a three-file edit with no compiler help if
you miss one.

This is the one place where "the layer model would suggest an
intervention" and the intervention is small: a shared scaffold builder
under `macros/src/include/` taking a description of which arrays and
submodules this role emits. It is not proposed here — it is a
`cratestack-macros`-scoped refactor ticket, and §7 leaves it there.

### 5.4 There is no shared client IR

`cratestack-client-dart` and `cratestack-client-typescript` have identical
dependency sets (`cratestack-core`, `cratestack-proto`, `minijinja`,
`serde`, `thiserror`) and thirteen identically-named source files, six of
which are parallel re-derivations of the same facts from
`cratestack_core::schema`: `naming.rs` (144 / 204 lines), `views.rs`
(162 / 241), `find_many_views.rs` (175 / 141), `context.rs` (206 / 183),
`config.rs` (125 / 81), `generator.rs` (52 / 55). Each crate walks the L0
IR and re-answers "what is this model's client-facing name / what views
does it have / what does its find-many input look like" in its own
idiom.

In layer terms: L4's outbound half has no L1. There is a contract — the
generated client surface — and it is written down twice, in Rust, in two
crates that cannot see each other.

`grpc-codegen-deduplication.md` Decision 3 (a proposal, not yet a decision)
already sketches the first slice of the fix — a `cratestack-client-grpc-shared`
crate for `GrpcMessageView`/`GrpcWireKind`/the collector fns — and
recommends it on precisely the precedent this document formalises:
`cratestack-sql` shared by `cratestack-sqlx`/`cratestack-rusqlite`. This
document does not reopen that; it names the general shape the proposal is
an instance of. Note that §5.6 and ADR 0013 argue the *same* precedent must
not be extended to runtimes; the distinguishing test is in ADR 0013.

### 5.5 Two small L1 purity breaches, both from #465

`cratestack-sql/src/idempotency.rs` (47 lines) holds
`IDEMPOTENCY_TABLE_DDL`, and its own module doc opens
`//! Idempotency DDL and utilities for Postgres.` The constant is 17 lines
of `BYTEA`, `TIMESTAMPTZ`, `UUID`, `NOW()` — unrunnable on SQLite — inside a
crate whose `lib.rs` opens `//! Dialect-agnostic SQL primitives`. It is a
genuinely small breach and #465 was a net improvement by a wide margin, but
the layer model makes it visible and it should either move to
`cratestack-sqlx` or be renamed to admit what it is.

Second: `cratestack-axum`'s entire dependency on `cratestack-sql` is
`crates/cratestack-axum/src/idempotency/store.rs:4` —
`pub use cratestack_sql::IDEMPOTENCY_TABLE_DDL;`, a backward-compatibility
re-export, and nothing else in `cratestack-axum/src` mentions
`cratestack_sql` at all. The edge is downward and therefore legal, but it
exists purely to preserve a public path. Pre-1.0, with #453/#454 having
shipped a breaking public-trait signature change directly the day before
(`32f89de`, 2026-08-07), that is a compatibility shim worth more than the
edge it buys.

A third, adjacent, and not previously recorded: `MAX_BODY_BYTES`
(`core/src/store/idempotency.rs:12`) is documented as the bound past which
"a request beyond this returns 413". A 413 is an HTTP status; an L4 concern
is living in an L1 contract module, and `cratestack-axum` re-aliases it
`pub(super)` at `idempotency/store.rs:8` anyway. ADR 0016 files this with
the other two.

### 5.6 `Dialect` has one method — and that is correct, but the name oversells

`cratestack-sql/src/dialect.rs` defines `trait Dialect` (line 14) with
exactly one method, `write_placeholder` (line 17). Its doc comment is not
apologetic about it:

> Kept deliberately narrow — adding methods here forces every backend to
> implement them, which is the wrong default. New dialect-specific quirks
> should live in the backend's own renderer until at least two backends
> agree on the shape.

That is a good rule and this document endorses it. But it means the honest
statement about L1 is narrower than "L1 is the backend contract":
`cratestack-sql`'s 2,799 lines are a shared *query-planning vocabulary*
plus one abstracted decision. Query execution, transactions, audit
emission, idempotency DDL, migrations and introspection are all written
per-backend — and `migrate/src/introspect/` is 972 lines across twelve
files, eleven of them under `introspect/postgres/` with no SQLite
counterpart, so `cratestack migrate diff` against a live database is
Postgres-only and any new backend inherits that asymmetry.

The layer model must not be read as a claim that a new backend is an L2
exercise. It is an L2 exercise *plus* whatever L1 has not yet abstracted,
which is most of it. Saying so is the point of naming the layers.

## 6. Spring, honestly

The framing that motivated this document: Spring's leverage is **runtime
indirection** — a `BeanFactory` plus CGLIB/JDK proxies. CrateStack's is
**compile-time macro expansion that monomorphises away**. The portable
ideas from Spring are its *seams*, not its container.

### What maps

**Boot starters → the three facades.** §4.1. CrateStack's version is
stronger, for the specific reason that Cargo resolves the graph before
`rustc` runs and `cargo tree` can prove a negative.

**Controller / Service / Repository → L4 / L3 / L2.** This is the sharpest
correspondence and also the most damning: CrateStack has a Controller tier
and a Repository tier and no Service tier, which is exactly why §5.1's
table looks the way it does. Idempotency and rate limiting sit in the
Controller tier because there was nowhere else; audit fan-out sits nowhere
for the same reason. The portable idea is not "add a service layer" as
ceremony — it is **the stratification tells you where a concern is
*allowed* to live**, and a concern with no legal home ends up in whichever
tier the author happened to be editing, or in no tier at all.

**Spring Data repositories → a Store SPI.** Partial. `ReadSource`/
`WriteSource` abstract *descriptor shape* (model vs view), not *storage
backend*; `IdempotencyStore`/`RateLimitStore`/`AuditSink` abstract backend
but only for three operational concerns, one of which has no consumer.
There is no `Repository<Model, PK>` seam and this document does not propose
one — see §8's Store SPI ADR for where that trade-off actually sits.

### What does not map, and must not

**`@Transactional`.** It works because a proxy wraps the bean. CrateStack
has no proxy point and no place to insert one; a transaction is a concrete
`sqlx::Transaction` threaded through generated code. Any attempt to
recover `@Transactional`'s ergonomics requires the thing this section
refuses.

**`@ConditionalOnClass` / auto-configuration.** The nearest analogue is a
Cargo feature, and Cargo features are *additive only* — you cannot
subtract a dependency by enabling a flag. `cratestack-pg`'s
`crypto-aws-lc-rs` feature is a hard `compile_error!` for exactly this
reason, documented in its own `Cargo.toml` (lines 121–132):
`cratestack-sqlx` and `cratestack-client-rust` hard-select the `ring`
rustls backend, and "additive Cargo features can't subtract that".
Conditional configuration does not have a compile-time twin, and pretending
otherwise produces features that silently do nothing.

**Component scanning.** No analogue, and `extensions.md` §7 already closed
the general case: `extension <name>` recognises a closed, framework-
maintained list, "not an arbitrary-extension mechanism".

### Why an IoC container is refused, permanently

Not "deferred". Refused, for four reasons that do not expire:

1. **It erodes monomorphisation**, which is CLAUDE.md's first paragraph.
   A container resolves by type at runtime; every resolved edge is a
   virtual call the optimiser cannot see through. §4.2's discipline —
   fourteen `dyn ` occurrences in five files, everything else concrete — is
   not an accident to be generalised, it is the design.
2. **It erodes the `db = None` guarantee.** A container needs a registry;
   a registry needs its implementations *reachable* to register them.
   `examples/no-database-verification` exists to prove `sqlx` and
   `libsqlite3-sys` are absent from the graph. A shared runtime backend
   registry makes that proof unstateable — not false, unstateable. The
   same argument kills any "runtime backend selection" design.
3. **The late binding buys nothing here.** Spring's container earns its
   cost when the wiring can vary without recompiling. CrateStack's schema
   *is a compile-time input*; there is no deployment where the binary is
   fixed and the wiring varies. You were going to recompile anyway.
4. **The 5% of the container people actually want is already there.**
   "I have N implementations and pick one at startup" is
   `Arc<dyn IdempotencyStore>`, at two working named seams, for the price
   of two vtables.

**Refusing the container is not refusing dependency injection.**
`Cratestack::builder(pool)` (`macros/src/include/server/runtime/postgres.rs:51`)
is constructor injection. `.layer(Extension(TrustedProxyConfig::…))` is
setter injection at the transport edge. Both are DI; neither needs a
container. The rejected thing is specifically *reflective, type-keyed,
runtime resolution* — and, as a corollary, any L3 design that reaches for a
registry of backend implementations. L3 must be a function over an
already-chosen set of collaborators, not a lookup.

## 7. What this paper does not decide

Deliberately left open, each to its own ADR (§8) or existing document:

- **Whether `OpExecutor` gets built now.** §5.1 itemises the cost of not
  having it; it does not overturn `rpc-transport.md` §6.5's gate. That is
  a maintainer call with a real "build speculative infrastructure" risk on
  the other side.
- **How far the Store SPI should go.** Freeze at three operational
  traits, or push toward a persistence SPI? §5.6 shows the second is much
  more expensive than `cratestack-sql`'s existence suggests.
- **Whether layer direction gets a CI check**, and in what form — including
  how ⊥ is expressed to a checker that only compares integers.
- **The two open client-side back-edges** (§3). Repairing them means moving
  `ClientStateStore` down out of `cratestack-client-rust/src/state.rs`, a
  breaking public-path change. That is a scoped ticket, not an
  architectural decision, and this paper does not schedule it.
- **The composer-scaffold consolidation** (§5.3) — a `cratestack-macros`
  refactor ticket, sized and scoped in its own PR.
- **Shared client IR** (§5.4) — already proposed by
  `grpc-codegen-deduplication.md` Decision 3. This paper adds vocabulary
  to that argument and takes nothing away from it.
- **Splitting `cratestack-core`.** It is the one crate that genuinely
  spans layers: `schema` is L0, `store`/`audit`/`codec` are L1, and
  `context`/`envelope`/`error`/`page`/`events`/`rpc`/`transport` and the
  rest are runtime vocabulary consumed at L2–L5. Twenty modules, three
  roles. The layer model names this as a **stated exception**, not a defect
  to be fixed: splitting it would touch every crate in the workspace to buy
  a tidier diagram, and the crate has zero workspace dependencies so it
  cannot itself violate the rule it complicates. If it is ever split, it
  should be because a real edge needs it, not because a table looks nicer.
- **A third database backend.** Nothing here is written to enable one.
  §5.6 exists to make the actual cost visible if someone proposes it.

## 8. ADRs — and where they live

**Correction to a premise this document was drafted against.** The claim
that CrateStack has no ADR convention is wrong. An ADR series exists,
numbered **0001–0005**, in the sibling documentation repository
(`cratestack-docs/internals/`), published at `cratestack.dev/internals/…`:

| # | File | Status |
|---|---|---|
| 0001 | `core-architecture-adr.md` | Proposed (updated 0.3.0, + RPC addendum) |
| 0002 | `mcp-operator-adr.md` | Proposed |
| 0003 | `views-adr.md` | Accepted |
| 0004 | `schema-diff-adr.md` | Proposed |
| 0005 | `rpc-transport-adr.md` | Accepted |

This is not decorative: **47 references to `ADR-0003`/`ADR-0004` appear in
this repository's own source** (46 to 0003, one to 0004) — in module docs
(`sql/src/descriptor/read_source.rs:61`, `parser/src/parse/views.rs:3`,
`migrate/src/diff/views.rs:1`, …), inside a `compile_error!` string
(`macros/src/include/embedded.rs:135`), in six crate READMEs and the root
README, and in the CHANGELOG. Code cites ADRs by number today.

So establishing `docs/adr/` in *this* repository is a new convention with
a live collision hazard, and it needs two rules:

1. **Continue the existing numbering.** New ADRs start at **0006**. Do not
   restart at 0001. A reader who greps `ADR-0003` must not find two.
2. **An ADR lives in the repo whose readers need it.** Product- and
   schema-surface decisions users must understand (views, RPC, migrations)
   stay in `cratestack-docs`, where they are published. Decisions about
   the *internal* shape of this workspace — layer boundaries, crate
   placement, enforcement — live here, in `docs/adr/NNNN-slug.md`,
   kebab-case per repo convention, cross-linked from `docs/design/`.

**Header format**, matching the existing series (`views-adr.md`,
`schema-diff-adr.md`) so the two homes read the same:

```markdown
# ADR NNNN: <Title>

## Status
Accepted | Proposed | Superseded by ADR NNNN

## Date
YYYY-MM-DD

## Context
<what forced the decision; cite files and prior docs>

## Decision
<what is decided, stated so a reader can apply it to a new case>

## Consequences
### Positive
### Negative
### Deferred
```

The six ADRs this document ships with, and the filenames they must land
under (an earlier draft generated them numbered 0001–0006, which is exactly
the collision rule 1 forbids):

| ADR | Filename | Status |
|---|---|---|
| 0006 | `docs/adr/0011-architecture-layer-model.md` | Accepted |
| 0007 | `docs/adr/0012-no-ioc-container.md` | Accepted |
| 0008 | `docs/adr/0013-facade-disjointness-invariant.md` | Accepted |
| 0009 | `docs/adr/0014-layer-direction-enforcement.md` | Proposed |
| 0010 | `docs/adr/0015-op-executor-l3.md` | Proposed |
| 0011 | `docs/adr/0016-store-spi-scope.md` | Proposed |

Two are `Accepted` because this paper settles them (the layer names and the
dependency rule; the permanent refusal of an IoC container). One is
`Accepted` because it restates an existing invariant that the layer model
puts under new pressure (facade disjointness). Three are `Proposed` because
a genuine trade-off remains: direction enforcement, `OpExecutor`, and the
reach of the Store SPI.

## 9. Verification notes

Everything numeric in this document was measured against `origin/main` at
`08fbb7e` (2026-08-08). Method, so it can be redone:

- Crate LoC: `git show origin/main:<path> | wc -l` summed over
  `git ls-tree -r --name-only origin/main crates/<crate>/src | grep '\.rs$'`.
  Includes in-crate `tests_*.rs`.
- Dependency edges: `[dependencies]` and
  `[target.*.dependencies]` sections of each `Cargo.toml`, filtered to
  `cratestack-*`. Dev-dependencies excluded — note that `cratestack-redis`
  gained a *dev*-dependency on `cratestack-axum` in #465 for its `Layer`
  types, which is not a layering violation but will show up in an
  unfiltered `cargo tree`.
- `ServerDb` count: `grep -cE 'ServerDb::(Postgres|None)'` over every file
  under `cratestack-macros/src` → 31 across 9 files (7 non-test); eight
  function signatures take `db: ServerDb`.
- `dyn` audit: per-file `grep -c 'dyn '` over `cratestack-axum/src`,
  excluding `tests*` files → 14 in 5 files.
- ADR references: `git grep -niE 'ADR-[0-9]{3,4}'` excluding lockfiles → 47.

Six claims that circulated during drafting and did **not** survive
checking, recorded so they do not circulate again:

1. "39 `match db` sites, worst `datasource_guard.rs` with 33." Actual: 31
   mentions across 9 files; `datasource_guard.rs` has 6 and is a
   single-purpose validation file. Corrected in §5.2.
2. "Three interchangeable rate-limit backends (in-memory / Postgres /
   Redis)." Actual: two production `RateLimitStore` impls — in-memory
   (`cratestack-axum`) and Redis. `cratestack-sqlx` implements
   `IdempotencyStore`, not `RateLimitStore`. This is not a drafting rumour:
   it is written in `extensions.md` §2 layer 3 and §5, and should be fixed
   there. Corrected in §4.2.
3. "`ReadSource` genericization is deferred to a follow-up PR." Its own
   module doc still says so; the code disagrees at twenty sites. Corrected
   in §4.3, and the doc comment should be fixed.
4. "As of #465 there are no back-edges." True of fourteen crates, false of
   thirty: `cratestack-client-store-{sqlite,redis} -> cratestack-client-rust`
   are L2 → L4 and unfixed. Corrected in §3.
5. "A crate at layer N may depend on layers < N." False of this document's
   own verification table (`sql -> policy`, `client-flutter ->
   client-rust`). Corrected to `≤ N` plus intra-layer acyclicity in §3.
6. "`AuditSink` is a working DI seam alongside the two store traits."
   Nothing calls `record()` except `MulticastAuditSink` itself; the
   generated builder has no method to install one. Corrected in §4.2.

## Reviewer notes

Adversarial pass before publication. What changed, and why.

**Three defects that would have been damaging once cited.**

1. **§3's rule was false of §3's own table.** The strict `< N` form forbids
   `cratestack-sql -> cratestack-policy` (both L1) and
   `cratestack-client-flutter -> cratestack-client-rust` (both L4), neither
   of which is a defect. Changed to `≤ N` with an intra-layer acyclicity
   condition, matching ADR 0011, which had already made this revision — the
   paper and its own ADR were shipping contradictory statements of the
   central rule.
2. **§3's "as of #465 there are no back-edges" was false.**
   `cratestack-client-store-sqlite` and `-store-redis` both depend on
   `cratestack-client-rust` (L2 → L4) purely to implement
   `ClientStateStore`, defined at `client-rust/src/state.rs:43` — the exact
   shape #465 fixed on the server side, still open. The original table
   enumerated fourteen of thirty crates and the claim was scoped to those.
   The full table is now printed, the two bad edges are marked, and the
   miss is used as the honest argument for §8's enforcement ADR rather than
   quietly repaired.
3. **§4.2 overstated `AuditSink`.** `record()` has exactly one caller
   (`MulticastAuditSink` fanning out to its own children);
   `cratestack-macros` never mentions the trait; the generated
   `CratestackBuilder` has one field and no way to install a sink. "Three
   DI seams" became "two working plus one reserved", and §5.1's audit row
   was split into persistence (correctly at L2, protected by a
   transactional invariant in `sqlx/src/audit.rs:1-5`) and fan-out
   (L3-shaped, no caller). This matters because the original §5.1 read as
   "audit is misplaced", which ADR 0015 then had to contradict.

**Baseline and numbers.** The paper claimed to verify against
`origin/main (cfde4e0)`; `origin/main` is `08fbb7e` and `cfde4e0` is its
parent. Re-pinned to `08fbb7e` and re-measured: `cratestack-axum` 4,849 →
5,212 (`08fbb7e` added 391 lines to it), `idempotency/layer.rs:17` → `:18`,
`ratelimit/layer.rs:15` → `:16`. The ADRs were split across both SHAs;
they now all pin `08fbb7e`. Added a merge-order note: this branch predates
#464/#465/#459, so two design docs the paper cites do not exist on it and
the `redis -> axum` back-edge is still live here.

**Smaller corrections.** `read_source.rs:8-13` → `:8-12`; "the ticket
sketch in its §5" → §6 of `idempotency-rate-limit-declarative-surface.md`;
"nine function signatures across six non-test files" → eight signatures,
and "six of them non-test" → seven; "one const, 20 lines" → a 17-line const
in a 47-line file; "four crate READMEs" → six plus the root; L4 table gained
the missing `cratestack-client-flutter` (553) and L2 gained its four
missing counts; `cratestack-sqlite -> cratestack-client-rust` (target-gated,
non-wasm) added to §3, since a mechanical checker must model `target` as
well as `kind`; §5.3's shared-scaffold list corrected (`MIXINS` is absent
from the client composer too); §4.1's "exist solely to hold a `cargo tree`
proof" softened (both examples also carry a working smoke test).

**Sourcing a correction properly.** §9's "three interchangeable rate-limit
backends" was recorded as a drafting rumour. It is not — it is written in
`extensions.md` §2 layer 3 and §5. Named, so the shipped doc gets fixed
rather than the error being absorbed silently.

**Additions.** §2 now says the layer table is illustrative and that ADR
0006 carries the complete per-crate assignment (five crates had no home in
either). §5.5 gained a third L1 breach, `MAX_BODY_BYTES` — a 413 bound
living in an L1 contract module. The ⊥ paragraph now states that a
compiler needs a *rule* rather than a number, because L5 depends on ⊥ and ⊥
must not depend on L2; a plain integer comparison cannot express that, and
ADR 0014 now carries it as an open question instead of assuming it away.
§8 gained the filename table that resolves the 0001–0006 numbering
collision for all six ADRs, not just the two that had noticed it.

**Scope.** Nothing here proposes code movement. §5.3 and §7 still decline
the composer refactor; the two client-side back-edges are named as a
defect and explicitly *not* scheduled. ADR 0016's three "finishing tickets"
are the one place the set edges toward file moves; that ADR now says
plainly that it records what its freeze implies and does not authorise the
moves.

**What held up.** Every commit SHA, every diffstat (`860c08b` 121 files
+11,221/−58; `6f14f1e` 27 files), the 47 ADR references, the 31 `ServerDb`
mentions, the 14 `dyn` occurrences, all facade line counts, the
`ReadSource`/`WriteSource` method counts (12+2 / 14), the `Dialect` doc
quote verbatim, the `crypto-aws-lc-rs` quote verbatim, the
`cargo deny`/`deny.toml` characterisation, the composer line counts
(239/238/190), the row-codec counts (226/206), the Dart/TS parallel-file
LoC pairs, and the entire `cratestack-docs` ADR 0001–0005 table including
statuses. The research this was built on was accurate on almost everything
it asserted; the failures were all of scope — claims true of a subset,
stated of the whole.
