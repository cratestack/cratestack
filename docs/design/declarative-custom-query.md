# Declarative, parameterized custom query in `.cstack` — spike

Status: **accepted** (2026-09-02) and **implemented** (cratestack#867).

The recommendation table below was accepted as written by the accountable
owner (@stephane-segning) in [epic cratestack#488's decision
comment](https://github.com/cratestack/cratestack/issues/488#issuecomment-5514756770),
which also answered the epic's open questions 1-4 in its terms. The spike
(cratestack#515) forbade implementation under itself; cratestack#867 is
the follow-up ticket that carried it, and shipped it.

> **Where the implementation deviated from this document, and why.** The
> spike deliberately left "exact keyword/attribute spelling" and the
> generated function shape as implementation-ticket decisions (§8). What
> cratestack#867 settled, recorded here rather than only in a PR body:
>
> - **Header spelling** follows `procedure`'s exactly —
>   `query <name>(<arg>: <Type>, ...): <ResultType>`, or `: <ResultType>[]`
>   for many rows — reusing the same argument parser. `T?` is rejected:
>   use `T` for one row or `T[]` for zero or more.
> - **The result type must be a `type` declaration.** A `model` is
>   rejected. §3 assumed a `type`; §6's soft-delete hazard is why it has to
>   be enforced rather than merely assumed — handing back a `Model` would
>   make a raw, unfiltered read look like the policy-filtered model read it
>   is not.
> - **Column names are the declared field names, verbatim**, which §3 left
>   open ("`AS this_month`, or `AS \"this_month\"` — resolved
>   per-implementation"). A `type` field `thisMonth` decodes from a column
>   named `thisMonth`, so the author's SQL must write `AS "thisMonth"`,
>   quoted, since Postgres folds unquoted identifiers to lower case. No
>   snake_case fallback: a decode that tries two names has a failure mode
>   that depends on which spelling the author happened to pick, which is
>   the opposite of §3's "fail loudly" position.
> - **Parameter types are restricted** to `String`, `Cuid`, `Int`,
>   `Float`, `Boolean`, `DateTime`, `Uuid` and `Bytes`, at required arity.
>   The spike did not scope this. `Decimal` is excluded because its Rust
>   type depends on the schema's `decimal =` backend (cratestack#505) and
>   whether that type implements `sqlx::Encode` depends on the backend
>   crate's feature set — a matrix that needs pinning down first. A money
>   *result* column is unaffected. Widening the list later is additive.
> - **Both an accessor and a free function.** §5 left the naming open;
>   §8 sketched a free `run`. Both exist: `db.queries().<name>(args, ctx)`
>   forwards to the module's own `run`, which is where the policy check
>   lives. That is a forwarder, not a second entry point — the §6 property
>   that matters is that no call shape *skips* the check, and none does.
> - **`db = None` gets its own guard**, which the spike did not anticipate:
>   a schema with no `datasource` block at all is legal and passes the
>   existing datasource guard, so `db = None` plus no datasource block
>   would otherwise reach codegen with queries intact.
>
> Everything in the recommendation table shipped as written.

> **Two things the spike missed entirely, found by cratestack#870's
> security review and fixed before merge.** Recorded here because both
> change what the construct *is*, not just how it is spelled.
>
> **A `query` body could write, and §6 did not notice.** This document's
> §6 reasons carefully about whether `@allow` can be bypassed and about
> which *rows* a read returns, and concludes the single-entry-point shape
> is safe. It never asks whether the body is a read at all. It is not:
> `WITH ins AS (INSERT … RETURNING …) SELECT …` is an ordinary `SELECT`
> to the driver, and it ran. The `@allow` gated the call, but the write
> bypassed `@@audit`, the `@@emit` outbox, `@version` optimistic locking,
> soft-delete, `@@internal` suppression and the target model's own write
> `@@allow` — an escape hatch around every guarantee the framework's own
> write path provides, reachable from a construct whose whole premise is
> that it only reads.
>
> The fix is not a SQL-text check. Classifying arbitrary SQL is the
> parsing §3 prices out and rejects, and a DML keyword blocklist is
> exactly the kind of check that looks right and is bypassable. Instead
> the generated `run` executes inside a Postgres `READ ONLY` transaction,
> so the engine refuses DML and DDL with SQLSTATE `25006` from the inside,
> whatever the statement looks like. Enforcement, not detection. **"A
> `query` reads only" is now a guarantee this document makes**, alongside
> the ones in §7.
>
> **A `query` does not observe an enclosing `db.transaction(...)`.** It
> runs on its own pooled connection, so a query called from inside that
> closure sees the pre-transaction state and, for a single-row query, can
> return `NotFound` for a row the closure just wrote. Composing the two is
> out of scope for v1 and is contradictory anyway now that the query's own
> transaction is `READ ONLY`. Documented on the generated `run` and pinned
> by a test that measures both halves — invisible before commit, visible
> after — so a future change here fails loudly instead of drifting.

Scope: `cratestack-parser` grammar, `cratestack-core` IR, `cratestack-macros`
codegen, and (for the "does it need one" question) the three client
generators, for a schema construct that lets `.cstack` express a
parameterized, typed, raw-SQL read that no existing construct can express.
Tracking: cratestack#515 (spike), part of epic cratestack#488, child story 1.

## Recommendation summary

| # | Question | Recommendation |
|---|----------|-----------------|
| 1 | Where the SQL lives | New top-level `query` block, modeled on `view`'s file/module layout but *without* DDL emission — not an extension of `procedure` or `view`. |
| 2 | Parameter declaration | Declared separately, exactly like `procedure`'s `(name: Type, ...)` arg list; bound positionally (`$1`, `$2`, ...); a parse-time scan validates the SQL body's `$N` references against the declared arg list — no SQL parsing, no type inference from SQL text. |
| 3 | Result typing | Author-declared, exactly like `view`'s declared `fields` block — not inferred. Decoded by column-name lookup at first execution (`sqlx::Row::try_get`), matching `view`'s own unchecked-until-runtime precedent. |
| 4 | Dialect scope | Postgres-only. A `query` block under `include_embedded_schema!` is a parse-time error. No `@@embedded_sql` twin in v1. |
| 5 | Client-surface consequence | None. Server-side only in v1 — no REST route, no RPC op ID, no Rust/Dart/TypeScript client stub. Callable only as a Rust function from code already running inside the server process. |
| 6 | Policy | `@allow`/`@deny` apply, evaluated against the query's own declared args (reusing the existing model-agnostic procedure-policy resolver), checked unconditionally inside the query's single generated entry point. The cratestack#512 `Authorized`-witness technique isn't structurally required *because* v1 has no second, user-implemented call shape to bypass around — but the same idiom should be pre-adopted as a forward-compatibility guard (detailed in §6). |
| 7 | v1 exclusions | No client surface, no SQLite/embedded backend, no result-shape inference, no named placeholders, no automatic soft-delete/`@allow`-row injection into the SQL body, no "unchecked" execution variant, no query builder/composable-filter surface. |

---

## 0. Re-verifying the issue's "Current Behavior" claims

The issue's four claims about `main` were re-checked directly against the
worktree at `ea03dc6` (branched from `main`), not re-derived from the
epic's paraphrase. All four still hold; two citations needed correcting to
the file that actually implements the claim (the underlying claim is
unaffected — this is a citation fix, not a retraction).

**1. "The aggregate builder handles one column and one aggregate per round
trip."** Holds. `crates/cratestack-sqlx/src/query/read/aggregate.rs:38-64`
(line numbers shifted from the issue's `41-61` — expected drift, noted
there too) exposes exactly `count()`, `sum<C>()`, `avg<C>()`, `min<C>()`,
`max<C>()` on `Aggregate<'a, M, PK>`, each taking one column and returning
a builder for one `SUM`/`AVG`/`MIN`/`MAX`. No two-aggregates-in-one-row,
no `FILTER (WHERE ...)`, no window function, no CTE — none of those have
any expression anywhere in this file or its siblings
(`aggregate_column.rs`, `aggregate_count.rs`).

**2. "`procedure` is signature-only — no body, no SQL."** Holds, more
strongly than the issue states: not only does
`crates/cratestack-parser/src/parse/procedures.rs:10-112` parse only
`name`, `args`, `return_type`, and generic `@`-prefixed attributes (no SQL
body syntax), but the IR type itself has no field for one —
`cratestack_core::Procedure` (`crates/cratestack-core/src/schema/procedure.rs:9-18`)
is `{ docs, name, name_span, kind, args, return_type, attributes, span }`.
There is no `body`/`sql` field to smuggle one into even informally; a SQL
body on `procedure` would be new IR, not an unused slot.

**3. View's `@@server_sql` compiles once into `CREATE VIEW` DDL at
migration time, with no runtime parameter substitution.** Holds, but the
issue's own citation
(`crates/cratestack-macros/src/include/server/collect.rs:196-214`) points
at the wrong file for *this specific claim* — I read it, and lines
196-218 there build the view's Rust-side struct/descriptor/accessor for
the ORM read path (`view_structs`, `view_descriptors`,
`view_pg_from_row_impls`, `view_accessors`), not DDL. The actual DDL
compilation is in `crates/cratestack-migrate/src/emit/postgres/views.rs:18-26`:

```rust
pub(super) fn emit_create_view(sql: &mut String, view: &CreateView) {
    writeln!(sql, "CREATE VIEW {} AS {};", quote_ident(&view.name), view.sql.trim()).unwrap();
}
```

`view.sql` is the raw string from `@@server_sql`
(`crates/cratestack-core/src/schema/view.rs:41-44`), spliced verbatim into
`CREATE VIEW ... AS <sql>;` once, at migration-emission time — a literal
string substitution with no `$N`/bind-parameter machinery anywhere in
that file or its siblings (`emit_replace_view`, `emit_create_materialized_view`).
Confirms "no runtime parameter substitution" for real, from the file that
actually does the compiling.

**4. "There is no `raw()`/`raw_sql` builder; `db.pool()` is the sole
exit."** Holds. Ran `grep -rn "fn raw" crates/cratestack-sqlx/src/` myself
— zero matches. `pub fn pool(&self) -> &sqlx::PgPool` is at
`crates/cratestack-sqlx/src/descriptor.rs:69-71` (issue cites `34-36`,
again drift — same file, correct claim).

---

## 1. Where the SQL lives

| Option | Blast radius | Verdict |
|---|---|---|
| **A. New `query` top-level block** (recommended) | New parser dispatch line + new `parse/queries.rs`; new IR type on `Schema`; new `cratestack-macros/src/query/` module mirroring `view/`'s layout (`descriptor.rs`, `accessor.rs`, `row_pg.rs`) *without* any `cratestack-migrate` involvement. **If v1 is server-internal-only (§5), zero changes to `transport/`, `axum/procedure.rs`, or any of the three client generators** — those emission sites simply never iterate a `query` list, the same way they never iterate `schema.views` today (confirmed below). | Recommended |
| **B. Parameterized extension of `view`'s `@@server_sql`** | Would require adding runtime bind-parameter substitution to a construct whose entire design — `CreateView`/`ReplaceView`/`CreateMaterializedView` DDL (`crates/cratestack-migrate/src/emit/postgres/views.rs`) plus the `ViewDescriptor`/`find_many()` read path (`crates/cratestack-sql/src/descriptor/view.rs`) — assumes the SQL compiles once into a persistent database object with a fixed, parameter-free body. | Rejected |
| **C. Body on `procedure`** | Reuses args/return-type parsing and the policy resolver, but every one of `procedure`'s five-plus existing emission sites (`collect.rs`'s `op_descriptor_entries`/`rpc_dispatch_arms` loops, `axum/procedure.rs`, and all three client generators) currently assumes *every* procedure is public surface, with **no existing suppression mechanism** — route/client-stub suppression is epic cratestack#488's own still-unshipped child story #5. Retrofitting an internal-only carve-out onto `procedure` means threading a new conditional through each of those sites. | Rejected as primary home |

**Recommendation: A.** The decisive fact for A vs. C: `collect.rs`'s
route/RPC-descriptor loops
(`crates/cratestack-macros/src/include/server/collect.rs:136-157`)
iterate `schema.procedures` and `schema.models` — **never `schema.views`**
— and no client generator (`crates/cratestack-macros/src/client.rs`,
`crates/cratestack-client-dart/src/*.rs`,
`crates/cratestack-client-typescript/src/*.rs`) has any code path keyed on
the `view` schema construct at all (the one incidental "view" grep hit,
`crates/cratestack-macros/src/client/rest/model.rs`, is an unrelated
`list_view`/`get_view` *model-projection* helper name — confirmed by
reading it, not a view-schema code path). A new `query` keyword that is
simply never looped over by those sites costs *less* than adding an
"is this procedure actually internal" branch to five call sites that have
never needed one. `view`'s existing internal-only shape is direct
precedent that this pattern already works in this codebase (§5 expands).

Rejected-alternative detail for **B**: a materialized view
(`@@materialized`) cannot take a runtime parameter in Postgres at all —
`CREATE MATERIALIZED VIEW` has no parameter slot, full stop — so
"parameterized view" is an outright contradiction for that half of
`view`'s surface, and for the non-materialized half it means bolting
caller-supplied-value substitution onto DDL-generation code
(`emit_create_view`) that has never had to reason about untrusted values,
where a raw-SQL escape hatch categorically must. Not a marginal cost —
a structural mismatch with what `view` *is*.

## 2. How parameters are declared and type-checked

| Option | Failure mode for a typo | Verdict |
|---|---|---|
| **A. Parse SQL at macro time to extract and type `$N`** | Extracting *positions* (`$1`, `$2`, ...) from raw text is a cheap scan. Extracting *types* is not: Postgres itself only knows `$1`'s type by preparing the statement against a live catalog — the same job `sqlx::query!`'s compile-time macros do by requiring `DATABASE_URL` at build time. `cratestack-macros` has never required a live DB connection at macro-expansion time for anything (models, views, procedures all resolve types from the `.cstack` text alone), and starting here would be a new, unprecedented compile-time dependency. | Rejected |
| **B. Declared separately, bound positionally** (recommended) | Args declared exactly like `procedure`'s `(name: Type, ...)` list (reusing `parse_procedure_args`, `crates/cratestack-parser/src/parse/procedures.rs:114-181, ` and `parse_type_ref`, `crates/cratestack-parser/src/parse/types.rs`). A new validator (mirroring `crates/cratestack-parser/src/validate/procedure.rs`'s pattern) scans the `@@sql(...)` body text for the `$N` substring pattern only — no SQL parsing — and checks two things: every `$N` referenced has `1 <= N <= args.len()`, and every declared arg is referenced by at least one `$N`. Both are compile-time `SchemaError`s. | Recommended |

Concrete failure messages (matching the existing style in
`crates/cratestack-macros/src/policy/procedure/resolver.rs`'s
`"unknown procedure input field \`{field}\` on \`{}\`"`):

- Out-of-range reference: `` query `loyaltyFeeSummary` references parameter `$3` in its SQL body, but only 2 parameter(s) are declared (`userId`, `cutoff`) ``
- Unreferenced declared arg (catches the "declared `cutoff` as `$2`, wrote `$3` by typo, `$2` now silently unused" case the issue worries about): `` query `loyaltyFeeSummary` declares parameter `cutoff` (`$2`) but it is never referenced in the SQL body ``

This directly answers the issue's framing concern — "a typo in `$2`
surfaces at runtime is a poor fit" — with a genuine no: because the
validator only needs the `$N` substring pattern, not full SQL
understanding, both directions of mismatch (SQL references an
undeclared index; a declared arg is never referenced) are caught during
schema parsing, which for `include_server_schema!` is macro-expansion
time, i.e. `cargo build`/`cargo check` time, not a running server.

**Positional, not named** — this also settles the epic's own open
question 4 (`$1` vs `:userId`) for this construct specifically: the
epic's own Risk table already flags named-parameter extraction as "new
parser surface and a plausible source of injection bugs if implemented
as string substitution." Positional avoids rewriting the SQL text at all
— `$N` is passed through to `sqlx::query_as(...).bind(...)` verbatim, in
declared-arg order, with no substitution step to get wrong. It also
matches what an author pastes straight from `psql`, which is the whole
point of an escape hatch.

## 3. How the result is typed

| Option | Cost | Verdict |
|---|---|---|
| **B. Inference from the `SELECT` list** | Priced honestly, per the issue's own instruction: the motivating query's `COALESCE(SUM(discount) FILTER (WHERE created_at >= $2), 0)::bigint AS total` requires understanding (1) that `::bigint` is an explicit cast whose target type wins outright, (2) `COALESCE`'s common-type-of-arguments rule, (3) that a `FILTER (WHERE ...)`-qualified `SUM` still needs `discount`'s underlying column type resolved by looking up the `loyalty_fee_events` model — i.e. a genuine SQL-expression type checker, spanning casts, aggregates, aliasing, and cross-model column lookups. `cratestack-sql` (`crates/cratestack-sql/src/lib.rs:1-16`) is explicitly a dialect-agnostic **AST/descriptor** crate — `SqlValue`, `Filter`, `OrderClause`, `ModelDescriptor` — with no facility for parsing arbitrary SQL text into an expression tree at all. Building one is a standing subsystem with ongoing maintenance cost tracking every Postgres expression/cast/function the framework chooses to support — exactly the "general-purpose query builder / reimplementation of SQL" the issue's own Out of Scope section rules out. | Rejected |
| **A. Author-declared result type** (recommended) | Zero new machinery — reuses `view`'s existing, already-shipped precedent (below) verbatim. | Recommended |

**Precedent, read directly from the code:** `view` already does exactly
this, today, and it is *not* cross-checked against the live `SELECT` list
at compile time either. `crates/cratestack-macros/src/view/row_pg.rs:38-48`
generates `sqlx::FromRow` decode as `#field_ident: row.try_get(#field_name)?`
— a column-name lookup against the declared Rust field name — for every
view field, whatever the view's own `@@server_sql` text actually
produces. A mismatch between a view's declared `fields` block and its
real `SELECT` list surfaces as `sqlx::Error::ColumnNotFound`/`ColumnDecode`
at first execution, not at compile time and not as a silently wrong
value — sqlx errors loudly on a missing/mistyped column, it doesn't
coerce. `query` should generate the identical decode shape (a
`generate_query_pg_from_row_impl` following `row_pg.rs`'s
`row_field_tokens` pattern near-verbatim), against the author's declared
result `type` (e.g. `type LoyaltyFeeSummary { total: Int64, thisMonth:
Int64 }`), matching column aliases the author writes in their own SQL
(`... AS total, ... AS this_month`, or `AS "this_month"` — resolved
per-implementation, not decided here).

Honest accounting of what this leaves unchecked: the issue's Expected
Result asks for "compile-time-checked parameters and a typed result."
Parameters *are* compile-time checked (§2). Result *shape* is
author-declared and Rust-side type-checked (the declared `type` is a
normal `.cstack` type, fully checked), but the *correspondence* between
that declared shape and the actual `SELECT` list is only checked at
first execution against real Postgres — identical to `view`'s existing,
shipped behavior, not a new gap this design introduces. Full compile-time
verification of that correspondence is exactly the inference cost priced
out above and is not pursued.

## 4. Dialect scope

**Recommendation: Postgres-only in v1.** A `query` block is legal only
under `include_server_schema!(db = Postgres)`; declaring one under
`include_embedded_schema!` is a semantic-check-time parse error, the same
way a `@@materialized` view without `@@server_sql` already is
(`crates/cratestack-parser/src/validate/views.rs:88-91`: "materialized
views are server-only — the embedded composer emits a hard compile error
when it encounters one").

Unlike `view`, which supports an explicit `@@server_sql` /
`@@embedded_sql` split so the *same* view can have two dialect-specific
bodies, `query` in v1 has **no `@@embedded_sql` twin at all** — not
because the split mechanism is hard to copy (it's the same `SQL_ATTRS`
pattern, `crates/cratestack-parser/src/parse/views.rs:18`), but because
the escape hatch's whole reason to exist is Postgres syntax a view's
simpler shape doesn't usually need: the motivating query's `FILTER
(WHERE ...)` and `::bigint` cast are Postgres-specific spellings a
generic "portable custom query" cannot credibly promise to translate.
This matches the epic's own framing exactly ("Portable is much more
expensive and probably wrong for a feature whose whole point is escaping
to real SQL — but say so explicitly") and epic Open Question 3's
SQLite-gap concern. `cratestack-sql`'s dialect-agnostic layer
(`Dialect`/`PostgresDialect`/`SqliteDialect`,
`crates/cratestack-sql/src/dialect.rs`) exists for the declarative
AST the framework already generates — it is not, and was never meant to
be, a target a hand-authored SQL string could be checked against.

If SQLite support is ever wanted, the honest shape (not proposed here,
named only so it isn't invented ad hoc later) is a second, independent
`@@embedded_sql`-style body on the same `query` block — the author writes
two dialect-specific SQL strings, same escape-hatch shape twice — never
one "portable" syntax. Out of scope for v1 (§7).

## 5. Client-surface consequence

**Recommendation: none in v1.** No REST route, no RPC op ID, no
Rust/Dart/TypeScript client stub. A `query` is reachable only as a Rust
function call from code already running inside the server process — a
`ProcedureRegistry` implementation, an `invoke_with_db`-driven background
worker, admin tooling.

This is not a new pattern invented for this design — it is `view`'s
existing, shipped behavior, verified directly:

- `crates/cratestack-macros/src/include/server/collect.rs:136-157` — the
  loop building `op_descriptor_entries` (RPC) iterates `schema.procedures`
  then `schema.models`; the loop building `route_transport_entries`
  (REST) iterates `pc.transport_entries` then `mc.transport_entries`
  (procedures, then models). Neither loop, nor any other in this file,
  ever touches `schema.views`.
- No client generator has a `view`-schema code path: `cratestack-macros/src/client.rs`
  dispatches purely on `models`/`procedures`;
  `cratestack-client-dart/src/*.rs` and
  `cratestack-client-typescript/src/*.rs` were grepped for `view` and the
  only hits are the unrelated `list_view`/`get_view` model-projection
  helpers (`crates/cratestack-macros/src/client/rest/model.rs`), confirmed
  by reading them.
- A view is reached exclusively via
  `runtime.views().<name>().find_many()`/`find_unique()`
  (`crates/cratestack-macros/src/view/accessor.rs:18-45`) — a server-side
  Rust accessor with zero HTTP-reachable counterpart.

`query` should follow the identical shape: a generated accessor (exact
naming — `runtime.queries().<name>(args, &ctx)`, a free function in a
`pub mod queries`, or similar — is an implementation-ticket decision, not
fixed here) callable only from Rust already inside the process. Blast
radius of *not* doing this: zero changes to `transport/{rest,rpc}.rs`,
`axum/procedure.rs`, or any file under `cratestack-client-rust/src`,
`cratestack-client-dart/src`, `cratestack-client-typescript/src` — all of
which currently assume every procedure/model they iterate is public
surface and would each need a new "skip this one" conditional if `query`
were made client-visible from day one. Exposing a client stub later, if
real demand appears, is strictly additive to this shape (new emission
sites reading the same IR), not a rework.

## 6. Policy — does `@allow` apply, and against what?

**Yes, mandatorily, evaluated against the query's own declared args.**
An empty policy list denies by default, matching the framework's existing
rule for models and procedures (which epic cratestack#488 itself lists as
explicitly Out of Scope to change).

There is no model to attach a policy to, but this generalizes cleanly:
`crates/cratestack-macros/src/policy/procedure/resolver.rs`'s
`resolve_procedure_field` (lines 14-55) already resolves policy
predicates purely against `Procedure.args`/the schema's `types` list — it
has **no model dependency at all**. The identical resolver logic applies
to `Query.args` (a `query` block's own arg list, structurally identical
to a procedure's) with no new machinery: `@allow(auth().subjectId ==
userId)` on a `query` block resolves exactly like the same predicate on a
`procedure` does today.

**Read cratestack#512/PR #540 before answering whether the technique
applies, per the ticket's instruction — I did.** The bypass PR #540 fixed
was structural: `ProcedureRegistry` methods are **user-implemented** Rust
(`registry.my_procedure(&db, &ctx, args)`), reached through a *separate*
generated wrapper (`invoke_with_db`,
`crates/cratestack-macros/src/procedure/instrument.rs:171-240`) that runs
`authorize_with_db` first — but nothing stopped a caller from calling the
registry method directly, skipping the wrapper and its policy check
entirely. The fix
(`crates/cratestack-macros/src/procedure/instrument.rs:28-67`,
`crates/cratestack-macros/src/procedure.rs:161-204`) makes the registry
method's signature require an `Authorized` witness — a zero-sized type
with a private tuple field, constructible only inside the same module as
`authorize_with_db`/`invoke_with_db` — so the policy-skipping call shape
fails to compile instead of compiling and silently bypassing policy.

**Why the witness isn't structurally required for `query` in this
design, and why to adopt the idiom anyway:** the witness exists to close
the gap between "where the check runs" (the wrapper) and "where the real
work runs" (user code in a different, directly-callable place). A `query`
block has **no user-implemented counterpart to `ProcedureRegistry`** — a
declarative query's execution is entirely generated code, not a trait an
author fills in. If v1 ships as a *single* generated entry point (e.g.
`pub async fn run(db, args, ctx)`), the policy check can be, and should
be, unconditional inside that one function's body — there is no second,
directly-reachable path for a caller to skip it *by construction*,
because no second path exists to skip to. That is a stronger default
than `procedure`'s pre-#512 shape had, not a weaker one, and it costs
nothing extra to keep — as long as v1 never adds an "unchecked"/raw twin
next to the checked entry point (§7 makes this an explicit exclusion, not
an oversight to be re-discovered later).

The forward-compatibility caveat: if a future revision ever splits query
execution the way `procedure` splits `authorize`/`invoke` (e.g. to let a
caller batch several queries' authorization together before running any
of them), that split re-creates exactly the two-call-shape gap #512
closed, and **must** adopt the identical `Authorized`-witness idiom at
that point — not a discretionary nice-to-have, a hard requirement, and
this document is where that requirement is now recorded so it isn't
missed. Recording this now, even though it doesn't bite in v1's
single-function shape, is cheap insurance against reintroducing the
exact bug class #512 just closed.

**Where this requirement now lives in the code.** It is written into the
module doc of `crates/cratestack-macros/src/query/entry.rs` — the file
that generates the single entry point, and therefore the file anyone
splitting it would be editing. A requirement recorded only in a design doc
is one nobody is reading at the moment they violate it.

One more policy-adjacent hazard, carried over honestly from epic
cratestack#488's own Risk table and *not* solved by anything above:
`push_scoped_conditions` (`crates/cratestack-sqlx/src/query/support/conditions.rs:35-90`)
is where soft-delete filtering and row-level `@allow` predicates get
injected into every *generated* read (`find_many`, aggregates, etc.). A
`query` block's SQL is raw text executed as-is — nothing injects a
`deleted_at IS NULL` predicate or a row-level policy filter into it. The
query's own `@allow` gates *whether the call is permitted at all*; it
does not filter *which rows* the query's `WHERE` clause matches. An
author querying a soft-delete-enabled model's table directly from a
`query` block owns every predicate themselves, including remembering the
soft-delete column — exactly the class of correctness bug the epic's own
Risk table names ("a soft-deleted row silently counting toward a
financial total is a correctness bug, not a style issue"). This is not
solved here; it's named explicitly in §7 as a documented hazard for v1,
with a possible compile-time lint suggested as future work, not required.

## 7. What v1 deliberately excludes

- **No client surface** (§5) — no REST route, RPC op ID, or generated
  Rust/Dart/TypeScript stub. Server-Rust-only.
- **No SQLite/embedded backend** (§4) — Postgres-only; a parse error under
  `include_embedded_schema!`; no `@@embedded_sql` twin.
- **No result-shape inference** (§3) — author declares the result `type`;
  the `SELECT`-list/declared-type correspondence is checked at first
  execution (matching `view`'s existing behavior), not at compile time.
- **No named placeholders, no string-interpolation binding** (§2) —
  positional `$N` only, passed straight to `sqlx::query_as(...).bind(...)`.
- **No automatic soft-delete or `@allow`-row filtering injected into the
  SQL body** (§6) — the author owns every `WHERE`/`FILTER` predicate.
  Documented hazard, not solved; a future compile-time lint flagging a
  `query` that references a soft-delete-enabled model's table without a
  matching predicate is plausible future work, not v1.
- **No "unchecked"/raw execution variant** bypassing `@allow` (§6) — no
  `db.pool()`-style deliberate escape hatch for `query` specifically;
  `db.pool()` itself remains available and unaffected, but a `query`
  block's own generated entry point always checks its policy.
- **No writes.** A `query` body executes inside a Postgres `READ ONLY`
  transaction, so `INSERT`/`UPDATE`/`DELETE`/`TRUNCATE` and DDL are
  refused by the engine — including inside a data-modifying CTE. Use a
  `procedure` or a model write builder for anything that changes data.
  Added after cratestack#870's review; see the header note for why this
  is enforced by the database rather than by inspecting the SQL.
- **No query-builder/composable-filter surface** — a `query` block's body
  is opaque raw SQL text end to end; nothing about `.cstack`'s `Filter`/
  `OrderClause` AST (`cratestack-sql`) applies to it. This is the epic's
  own Out-of-Scope framing restated for this construct: an escape hatch
  to real SQL, not a reimplementation of SQL in `.cstack`.
- **No composition with `db.transaction()`.** *(Corrected 2026-09-02:
  when this spike was written it said "no `db.transaction()` combinator",
  which was already false — cratestack#513 shipped it in PR #539, and
  cratestack#488's own 2026-09-02 comment retracts the same stale claim.
  The combinator exists; what is out of scope is a `query` participating
  in one.)* A query runs on its own pooled connection and cannot see an
  enclosing transaction's uncommitted writes. Read after it commits.
- **No `ProcedureRegistry`-bypass fix beyond what #512/#540 already
  shipped, and no route-suppression work** — the latter shipped
  separately as cratestack#743.
- **No migration/DDL involvement** — a `query` block never appears in any
  `cratestack-migrate` output; unlike `view`, there is no persistent
  database object to create, replace, or drop.

## 8. The motivating query, end to end

Schema declaration (illustrative syntax — exact keyword/attribute
spelling is an implementation-ticket decision, not fixed by this spike):

```cstack
type LoyaltyFeeSummary {
  total       Int64
  thisMonth   Int64
}

query loyaltyFeeSummary(userId: String, cutoff: DateTime): LoyaltyFeeSummary
  @@sql("""
    SELECT
      COALESCE(SUM(discount), 0)::bigint AS total,
      COALESCE(SUM(discount) FILTER (WHERE created_at >= $2), 0)::bigint AS this_month
    FROM loyalty_fee_events
    WHERE user_id = $1
  """)
  @allow(auth() != null && auth().subjectId == userId)
```

Parameter validation at parse time (§2): `$1` and `$2` both appear in the
SQL body, matching the two declared args in order — no error. If the
author instead wrote `$3` in the `FILTER` clause (a typo for `$2`), the
schema fails to compile with `` query `loyaltyFeeSummary` references
parameter `$3` in its SQL body, but only 2 parameter(s) are declared ``.

Generated signature (illustrative — mirrors `view`'s
`row_pg.rs`/`accessor.rs` shape and `procedure`'s
`authorize`/`authorize_with_db` naming, per §6):

```rust
pub mod queries {
    pub mod loyalty_fee_summary {
        pub struct Args { pub user_id: String, pub cutoff: DateTime<Utc> }

        #[derive(sqlx::FromRow)]
        pub struct Row { pub total: i64, pub this_month: i64 }

        const SQL: &str = "SELECT COALESCE(SUM(discount), 0)::bigint AS total, \
            COALESCE(SUM(discount) FILTER (WHERE created_at >= $2), 0)::bigint AS this_month \
            FROM loyalty_fee_events WHERE user_id = $1";

        pub async fn run(
            db: &super::super::Cratestack,
            args: Args,
            ctx: &::cratestack::CratestackContext,
        ) -> Result<Row, ::cratestack::CratestackError> {
            ::cratestack::authorize_procedure(ALLOW_POLICIES, DENY_POLICIES, &args, ctx)?;
            sqlx::query_as::<_, Row>(SQL)
                .bind(&args.user_id)
                .bind(args.cutoff)
                .fetch_one(db.pool())
                .await
                .map_err(::cratestack::CratestackError::from)
        }
    }
}
```

Call site, from inside a procedure implementation or a background worker
(the latter using `auth().isSystem()`, cratestack#486, for identity —
already shipped, out of scope here):

```rust
let summary = cratestack_schema::queries::loyalty_fee_summary::run(
    &db,
    cratestack_schema::queries::loyalty_fee_summary::Args {
        user_id: ctx.subject_id().expect("authenticated").to_string(),
        cutoff: month_start,
    },
    &ctx,
)
.await?;
```

### Self-test against further queries, per the epic's own Risk instruction

- **Zero-parameter query** (`SELECT COUNT(*) FROM loyalty_fee_events WHERE
  created_at >= NOW() - INTERVAL '1 day'`): `args = []`, no `$N` in the
  body; the validator's range check (`1..=0`) is vacuously satisfied.
  No special case needed.
- **List-returning query** (a `GROUP BY` summarizing multiple users):
  `-> LoyaltyFeeSummary[]` reuses `TypeArity::List`
  (`crates/cratestack-core/src/schema/model.rs:127-131`), already how
  procedures declare `T[]` returns
  (`crates/cratestack-macros/src/procedure/instrument.rs:143-151`'s
  `OpKind::Sequence` handling) — the generated function calls
  `fetch_all` instead of `fetch_one`; no new arity concept required.
- **CTE / window-function query** (`WITH recent AS (...) SELECT
  ROW_NUMBER() OVER (...) ...`): passes through unchanged — the validator
  only scans for `$N` tokens, never SQL structure, so nothing about CTEs
  or window functions is restricted or specially handled.
- **Soft-delete hazard case** (`SELECT SUM(amount) FROM orders WHERE
  customer_id = $1` where `orders` has `@@soft_delete`): compiles and
  runs, but — per §6 — silently includes soft-deleted rows unless the
  author adds `AND deleted_at IS NULL` themselves. Confirms §6/§7's
  documented-hazard framing is not hypothetical; it reproduces on the
  very first non-trivial query tried beyond the motivating one.

## Non-goals

- A general-purpose SQL parser or expression type-checker in
  `cratestack-sql` or `cratestack-parser` (§3).
- Portable (Postgres + SQLite) custom-query syntax (§4).
- A client-facing custom-query surface in v1 (§5) — plausible future work
  if real demand appears, not pursued here.
- Automatic row-level policy/soft-delete injection into raw SQL bodies
  (§6/§7) — the author owns every predicate.
- Any change to `view`'s `@@server_sql`/DDL semantics — `query` is a
  disjoint new construct, not an extension (§1), matching epic
  cratestack#488's own Out-of-Scope framing on this point.
