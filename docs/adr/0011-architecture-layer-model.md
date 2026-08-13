# ADR 0011: The CrateStack Layer Model and the Downward Dependency Rule

## Status

Accepted

## Date

2026-08-08

Context doc: [docs/design/layering.md](../design/layering.md)

## Context

CLAUDE.md states the crate graph as a single line — "parser → core/policy/sql → macros →
backend runtimes / clients". That line is load-bearing: PR #465 (`6f14f1e`,
"fix(cratestack-core,cratestack-sql): move storage traits and DDL from cratestack-axum to
core/sql") cites it verbatim as the rule that had been violated. It is also no longer
sufficient. `git ls-tree -r --name-only origin/main crates | grep -c 'Cargo.toml$'` returns
**35** (34 of them workspace members; `cratestack-studio-ui` is excluded). A five-name chain
does not tell you where `cratestack-grpc`, `cratestack-proto`,
`cratestack-client-store-sqlite` or `cratestack-mock-wiremock` belong, and a rule you cannot
apply to a new crate without asking the maintainer is a habit, not an architecture.

The defect #465 fixed was structural, not stylistic: `IdempotencyStore` and `RateLimitStore`
were *defined* in `cratestack-axum`, so `cratestack-sqlx` and `cratestack-redis` — two storage
adapters — depended on an HTTP transport crate for no reason other than trait location. The
traits moved to `cratestack_core::store::{idempotency,ratelimit}` and `IDEMPOTENCY_TABLE_DDL`
to `cratestack-sql`. Verified at `origin/main` (`08fbb7e`): `cratestack-sqlx -> core, policy,
sql` and `cratestack-redis -> core`.

Nothing caught it, and nothing would catch the next one. `cargo deny check` is the only
dependency gate in `just all-checks`, and `deny.toml`'s entire `[bans]` section is
`multiple-versions = "warn"`. It has no concept of intra-workspace direction and is not
meant to.

`docs/design/layering.md` names six strata plus an orthogonal compiler and argues them at
length. This ADR adopts that vocabulary as binding. Three of its claims did **not** survive
re-verification against `origin/main`, and this ADR settles them rather than inheriting them
(the context doc has since been corrected to match):

1. **Same-layer edges exist, and the paper's original rule forbade them.** An earlier draft
   of §3 stated "a crate at layer N may depend on crates at layers < N, and on nothing at
   layer ≥ N", then presented a table containing `cratestack-sql -> core, policy` — while §2
   places *both* `cratestack-sql` and `cratestack-policy` at L1. The rule as written was
   false of its own verification table. `cratestack-client-flutter -> cratestack-client-rust`
   is a second instance (L4→L4).
2. **"As of #465 there are no back-edges" was verified over 14 crates, not 30.** Among the
   omitted ones is a live mirror of exactly the defect #465 fixed: `ClientStateStore` is
   defined at `crates/cratestack-client-rust/src/state.rs:43` (a binding), and both
   `cratestack-client-store-sqlite` and `cratestack-client-store-redis` (adapters) depend on
   `cratestack-client-rust` solely to implement it. Same shape, client side, unfixed.
3. **One legal edge was missing from the table entirely.** `cratestack-sqlite` depends on
   `cratestack-client-rust` under `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`
   (`crates/cratestack-sqlite/Cargo.toml:63-64`), for hybrid consumers that ship an embedded
   DB *and* call a remote service. It is L5 → L4 and legal, but it means any statement about
   "the `[dependencies]` graph" has to include target-gated sections.

Also verified for this ADR: no crate at any layer depends on `cratestack-{cli,lsp,migrate,
studio,mock-wiremock}` — the only edges into those are tool→tool (`cli -> migrate`,
`cli -> studio`, `cli -> mock-wiremock`, `studio -> migrate`). `grep OpExecutor` over
`origin/main` hits `CHANGELOG.md` and three files under `docs/design/`, and zero lines of code.

## Decision

**CrateStack organises its crate graph into six numbered layers, one orthogonal compiler, and
a set of tools, and enforces a downward-only dependency rule between them.**

**1. The rule.** A crate at layer N may depend on crates at layers **≤ N**, and on nothing at
a layer **> N**. Same-layer edges are permitted provided the intra-layer graph stays acyclic;
only *upward* edges are forbidden. This is the minimum revision to `layering.md`'s original
formulation that makes the rule true of the actual graph (it legalises `sql -> policy` and
`client-flutter -> client-rust`) while still catching every defect the strict form catches —
`sqlx -> axum` (fixed by #465) and `client-store-* -> client-rust` (open) are both upward, and
both remain violations. Tools may depend on anything; nothing at L0–L5 or ⊥ may depend on a
tool. **⊥ is governed by a rule, not a number:** `cratestack-macros` may depend on L0 and L1
only, and L5 may depend on ⊥. That relation cannot be expressed as an integer comparison —
any number low enough to forbid `macros -> sqlx` is too low for `pg -> macros` under `≤`. ADR
0009 carries the consequences of that for a mechanical checker; this ADR states the rule.

**2. The assignment.** Every crate at `origin/main` has exactly one home:

| Layer | Crates |
|---|---|
| **L0** Schema IR | `cratestack-parser`, `cratestack-core::schema` |
| **L1** Contracts | `cratestack-sql`, `cratestack-policy`, `cratestack-core::{store,audit,codec}`, `cratestack-auth` |
| **L2** Adapters | `cratestack-{sqlx,rusqlite,redis}`, `cratestack-codec-{cbor,json}`, `cratestack-client-store-{sqlite,redis}`, `cratestack-outbox`, `cratestack-service` |
| **L3** Execution | *(empty — see commitment 4)* |
| **L4** Bindings | `cratestack-{axum,grpc}`, `cratestack-client-{rust,dart,typescript,flutter}`, `cratestack-cbor-{napi,wasm}` |
| **L5** Facades | `cratestack-{pg,api,sqlite,client}` |
| **⊥** Compiler | `cratestack-macros`, `cratestack-proto` |
| Tools | `cratestack-{cli,lsp,migrate,studio,studio-ui,mock-wiremock}` |

(`cratestack` is the documentation-only vitrine crate — its `src/lib.rs` exports no items —
and has no layer.) A new crate must be assigned a layer in its introducing PR. The test is
behavioural, not topical: L1 is where a contract two layers must agree on lives; L2 is where
knowledge of one specific external system lives; L4 is where one wire protocol's
encode/decode/route/status mapping lives.

**3. Two stated exceptions, recorded rather than fixed.**

- `cratestack-core` deliberately spans layers: `schema` is L0, `store`/`audit`/`codec` are L1,
  and `context`/`envelope`/`error`/`page`/`events`/`rpc`/`transport` and the rest of its
  twenty modules are runtime vocabulary consumed at L2–L5. It has **zero** workspace
  dependencies, so it cannot itself violate the rule. It is **not** proposed for splitting.
- L5 parameterises ⊥ **upward** via forwarded Cargo features — `cratestack-pg`'s `postgres`,
  `grpc`, `rate_limit` and `pgvector` each forward to a same-named feature on
  `cratestack-macros`, read via `cfg!(feature = "...")` against the macro crate's *own*
  compiled feature set (`docs/design/extensions.md` §2, after the `CARGO_FEATURE_<NAME>`
  approach was empirically disproved in #161). This is information flow, not a dependency
  back-edge, and it is legal.

**4. L3 is named and left empty.** Naming it is the point: it makes "idempotency and rate
limiting fire from L4 `tower::Layer`s, row-level policy fires from inside generated SQL, and
audit fan-out fires from nowhere at all" a sentence that can be said, and therefore a
situation that can be argued about. Nobody may create a crate merely to fill the slot.
Whether `OpExecutor` gets built is a separate, still-open decision, gated in
`docs/design/rpc-transport.md` §6.5 and handed to ADR 0015.

**5. This ADR decides placement and direction only.** It does not decide whether the rule gets
a CI check (ADR 0014), whether `OpExecutor` is built (ADR 0015), or how far the Store SPI
should reach (ADR 0016). Each is a genuine trade-off and belongs to its own ADR.

## Consequences

### Positive

- A new crate's placement is answerable from the crate's own behaviour, without the maintainer.
- The rule is falsifiable by a mechanical procedure (`cargo tree -i`, or reading
  `[dependencies]` filtered to `cratestack-*`), which is what made #465 findable at all.
- `layering.md`'s three overstated claims are corrected in the record instead of propagating.
- The L5 → ⊥ feature-forwarding exception is now documented as legal, so nobody "fixes" it.

### Negative

- **The invariant ships red.** Adopting this ADR converts `client-store-sqlite -> client-rust`
  and `client-store-redis -> client-rust` from an unnamed situation into a named violation with
  no scheduled fix. Repairing it means moving `ClientStateStore` down out of
  `cratestack-client-rust/src/state.rs` — a breaking public-path change, affordable pre-1.0 but
  real churn, and the exact category of shim §5.5 of the context doc already flags in
  `cratestack-axum/src/idempotency/store.rs:4`.
- **Edge assignments are judgment calls and will be argued.** `cratestack-cbor-napi` is placed
  at L4 because it binds an L2 codec to a foreign runtime, but it speaks no wire protocol; a
  reviewer could reasonably call it a tool. The table resolves such cases by fiat, not by
  derivation.
- **⊥ resists the numbering the rest of the model uses.** The compiler needs a bespoke rule
  because it sits below L1 for its own dependencies and above L4 for its consumers. That is a
  permanent special case, and it is the one place the model is not a total order.
- **Naming an empty layer invites building it.** L3 will read to some as a backlog item. This
  ADR forbids that, but the temptation is a cost the model creates and did not previously exist.
- **Cross-cutting features get more expensive to justify.** `@@subscribe`/SSE (`c0a76d1`)
  touched 34 files across seven crates; under this ADR each crossing now needs a defence.
- **It forecloses the cheap fix.** "Define the trait where it is used" — the move that produced
  the #465 defect — is now illegal even when it is the smallest diff. It also forecloses any
  shared runtime registry of backend implementations, which would make the
  `examples/no-database-verification` proof of absence unstateable rather than merely false.
- Same-layer edges being legal means the rule cannot be checked by a simple integer comparison
  alone; a checker must also assert intra-layer acyclicity, model target-gated dependency
  sections, and special-case ⊥.

### Deferred

- Mechanising the direction check in CI — ADR 0014.
- Whether `OpExecutor` (L3) is built now — ADR 0015; `rpc-transport.md` §6.5's gate stands.
- How far the Store SPI reaches beyond the three operational traits — ADR 0016.
- The client-side back-edge repair — a scoped ticket, not an architectural decision.
- The composer-scaffold consolidation (`layering.md` §5.3) and shared client IR (§5.4) —
  `cratestack-macros`-scoped refactor work and `grpc-codegen-deduplication.md` Decision 3
  respectively; neither is reopened here.

**Revisit this ADR if:** a same-layer cycle appears; a second layer turns out empty; or
`cratestack-core`'s span stops being a diagram wart and becomes a blocking edge.

**Cross-reference note (added by ADR 0014's implementation, 2026-08-08).** This ADR's
decision 1 states "⊥ is governed by a rule, not a number" and its table lists `cratestack-
macros`/`cratestack-proto` under a separate `⊥` row. That remains the correct *prose*
description — ⊥ is a compiler that emits into L2 through L5, not a layer those crates pass
through. It turned out not to require a distinct *numeric* rule for the mechanical checker:
ADR 0014 verifies that both crates' real workspace dependencies are entirely L0/L1, so
`docs/adr/layers.toml` (the checker's input) assigns them `L1` directly, with no special
predicate, rather than the `compiler` role ADR 0014 originally proposed. See ADR 0014's
Amendment section for the full argument. This ADR's own table and prose are unchanged; only
the checker's encoding of the same fact is simpler than either document originally assumed.

## Alternatives considered

**Keep CLAUDE.md's one-line flow and fix violations as found.** Strongest case: it costs
nothing, adds no ceremony, and it *worked* — a human running `cargo tree -i` produced #465.
The workspace is small enough that one maintainer can hold the whole graph. Rejected because
the enforcement mechanism is "someone notices", and the verification above shows it missed the
client-side twin of the very defect it caught, which has been sitting in the graph unremarked.

**Fewer layers: fold L1 into L2 and drop the empty L3.** Strongest case, and this was the
closest call: the model would then be purely descriptive with no empty slot to explain, and
L1 is genuinely thinner than "the backend contract" implies — `cratestack-sql`'s `Dialect`
trait has exactly one method, `write_placeholder` (`crates/cratestack-sql/src/dialect.rs:17`),
while execution, transactions, migrations and introspection are all written per-backend.
Rejected because the empty slot is the model's most useful product. A model that only
describes cannot diagnose, and the L3-shaped hole is precisely what explains why
`@no_idempotency` codegen has been deferred across two release cycles
(`idempotency-rate-limit-declarative-surface.md` §4.2/§6) and why row-level `@@allow` is not
replayed against streamed events (`rpc-transport.md` §3.4a, restated in §6.5).

**Split `cratestack-core` so the assignment is mechanical.** Strongest case: it is the one
crate that truly spans layers, and a `schema` / `contracts` / `runtime-vocabulary` split would
remove the largest exception in this ADR and let any future checker work at crate granularity
with no special cases. Rejected: it touches every crate in the workspace to buy a tidier
diagram, and `cratestack-core` has zero workspace dependencies — it cannot violate the rule it
complicates. Recorded as a stated exception; split it only when a real edge demands it.

**Enforce structurally — nested workspaces or per-layer `[bans]` — instead of by convention.**
Strongest case is the honest one: an invariant nobody checks decays, and separate per-layer
workspaces would make an upward edge simply unresolvable rather than merely discouraged.
Rejected *here* on two grounds: it is a different decision with its own cost/benefit and is
handed to ADR 0014, and splitting the workspace would break the lockstep `just bump` /
`just release` topo-sort that the release process depends on.

**Organise around a container instead of a stratification.** Strongest case: Spring's
Controller/Service/Repository split is the closest existing analogue to what this ADR
describes, and a container hands you the substitution seams for free rather than requiring
each one to be designed. Not rejected here — the permanent refusal of an IoC container is
ADR 0012 — but noted as the alternative this model is the residue of: CrateStack takes
Spring's *stratification*, which tells a concern where it is allowed to live, and refuses its
*container*, whose runtime type-keyed resolution would erode both monomorphisation and the
`db = None` dependency-surface guarantee.
