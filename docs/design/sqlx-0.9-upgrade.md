# Upgrading sqlx 0.8.6 → 0.9 — investigation

> **SUPERSEDED — the upgrade was carried out, and this document's estimate
> was measurably wrong. Read this box before relying on any figure below.**
>
> The verdict ("upgrade when ready, not blocked") held, and the pgvector
> analysis in §2 was correct and load-bearing. The *sizing* was not:
>
> | Claim below | Actual |
> |---|---|
> | "12 call sites across 4 files" need `AssertSqlSafe` (§4, §6) | **17 sites.** The doc missed the `cratestack-studio` test fixtures, `cratestack-pg/tests/total_count_aggregate.rs`, and two `Executor::execute(&str)` sites now needing `&'static str` |
> | "**0 public-signature rewrites**" (§6) | **32 signature edits.** `QueryBuilder` lost its lifetime parameter — `QueryBuilder<'_, Postgres>` → `QueryBuilder<Postgres>` — plus three `fn build_query<'q>` lifetimes that then became unused |
> | (not mentioned) | `QueryBuilder::sql()` now returns an owned `SqlStr` with no `Deref`/`Display`, so `.sql().to_owned()` / `sql.contains(..)` became `.into_string()` |
>
> `cratestack-sqlx` alone produced **35 compile errors**, not the handful
> implied here. The `QueryBuilder` lifetime cascade was the miss: this
> document flagged the underlying `Arguments` lifetime removal as
> low-risk, and it was the single largest source of edits.
>
> Recorded rather than quietly fixed, per this repo's own doctrine on
> owning errors in the place they were made. The rest of the document is
> left as the dated investigation record it is.

Status: **investigation only, no maintainer decision made**. This document
is the deliverable of a research spike; it does not implement anything, and
nothing in the workspace `Cargo.toml`/`Cargo.lock` changed to produce it.
Scope: whether/when to move the workspace's `sqlx-core`/`sqlx-postgres`
pins (currently `=0.8.6`) to the `0.9` line, and what that touches.
Tracking: no issue exists yet for this work. Source of truth for the
version claims below is the upstream sqlx release itself —
[CHANGELOG.md @ `launchbadge/sqlx`](https://github.com/launchbadge/sqlx/blob/main/CHANGELOG.md)
and the crates.io registry API, both fetched directly for this
investigation (not recalled from training data — see §0).

## Verdict

**Upgrade when ready, not blocked.** The one blocker this investigation was
told to check first — pgvector's sqlx compatibility — turned out to already
be resolved (pgvector 0.4.2, released one day after sqlx 0.9.0, explicitly
adds sqlx 0.9 support; see §2). MSRV is not raised past what CI already
requires (§5). The workspace's underscore-prefixed "private" sqlx-core
features are unchanged in 0.9.0 (§6). The actual work is small and
mechanical: 12 call sites across 4 files need `AssertSqlSafe(...)` wrapping
for a new trait bound (§4), plus the two-file, four-line lockstep version
bump the existing `=0.8.6` pin already requires for any version change
(§3). This is a same-day PR, not a project. No reason found to do it
*today* over other work, but also no reason found to defer it once someone
has a slot — recommend picking it up as a normal-priority ticket rather
than leaving it pinned indefinitely.

## 0. What's actually current, verified against the registry

`sqlx`, `sqlx-core`, and `sqlx-postgres` all show `max_version: 0.9.0` on
crates.io as of this investigation (queried directly against
`https://crates.io/api/v1/crates/<name>`, not assumed). Release history for
`sqlx-core`, most recent first:

```
0.9.0         2026-05-21
0.9.0-alpha.1 2025-10-15
0.8.6         2025-05-19   ← current workspace pin
0.8.5         2025-04-15
0.8.4         2025-04-14   (yanked)
0.8.3         2025-01-04
```

The upstream CHANGELOG's own header dates 0.9.0 "2026-05-06", ~2 weeks
before the crates.io publish timestamp — not a discrepancy that matters
here, just noting both dates exist in different places. There is no 0.9.1
or later; 0.9.0 is the current tip of the 0.9 line, seven and a half months
after the alpha.

Full 0.9.0 changelog: 379 lines, `## 0.9.0 - 2026-05-06` through the link
reference block, fetched from
`https://raw.githubusercontent.com/launchbadge/sqlx/main/CHANGELOG.md`
(the file at `main`, which is a maintained cumulative changelog, not a
per-tag diff). Quoting the parts that matter to this repo below; the full
text also covers MySQL/SQLite-only changes this repo never compiles
(cratestack uses `sqlx-postgres` only — the embedded backend is `rusqlite`
directly, not `sqlx-sqlite` — see `CLAUDE.md`'s "three-macro / role model").

Also notable, non-technical: the sqlx repo is moving from `launchbadge` to
a new GitHub org (`transact-rs`) "shortly after" the 0.9.0 release,
because it hasn't been LaunchBadge-maintained for years. Doesn't affect
crates.io coordinates or this investigation's conclusions, but a future
implementer following links should expect the canonical repo URL to move.

## 1. Why the pin is `=0.8.6`, not `^0.8.6` — and why it still applies at 0.9

The exact-equality pin was introduced in the same commit that moved this
repo off the `sqlx` umbrella crate onto `sqlx-core` + `sqlx-postgres`
directly (`fcc399f`, 0.3.0, PR #9) — `git log -S'=0.8.6' -- Cargo.toml`
shows no later commit touching that literal string. But the *reason* for
equality (not just directness) is documented in
`crates/cratestack-sqlx/Cargo.toml:81-91`, on the crate's own `sqlx = {
version = "=0.8.6", ... }` dependency (a dependency this crate never
imports from directly — it's declared purely to anchor the resolver):

> Pins the transitive `sqlx` facade crate pgvector's `sqlx` feature pulls
> in to exactly the version whose `sqlx-core`/`sqlx-postgres` match our own
> direct `=0.8.6` pins above. Without this, pgvector's own (loose) `>= 0.8,
> < 0.10` requirement lets Cargo resolve a separate, newer
> `sqlx-core`/`sqlx-postgres` (e.g. 0.9.x) alongside our pinned 0.8.6 ones,
> and `pgvector::Vector`'s `Encode`/`Type` impls (written against the newer
> copy) then fail to satisfy `QueryBuilder::push_bind`'s bound (written
> against ours) — not a missing impl, just two incompatible copies of the
> same trait.

Verified: pgvector 0.4.2's own dependency spec (fetched from
`https://crates.io/api/v1/crates/pgvector/0.4.2/dependencies`) is `sqlx
>=0.8, <0.10` for the optional `sqlx` feature — a range wide enough to
admit 0.9.x today, which is exactly the duplicate-copy hazard the pin's
comment describes. **This means the reasoning for an exact pin survives
the 0.9 upgrade unchanged**: bumping the workspace root's `sqlx-core`/
`sqlx-postgres` to `=0.9.0` without *also* bumping
`crates/cratestack-sqlx/Cargo.toml:92`'s `sqlx = "=0.8.6"` anchor to
`=0.9.0` in the same commit would silently reintroduce the two-copy
problem this pin exists to prevent. Both edits are two lines in two files
(`Cargo.toml:331-332` at the workspace root, `crates/cratestack-sqlx/
Cargo.toml:92`), but they are two files, and a future implementer who
only edits the workspace root (the more visible pin) would leave the trap
in place.

## 2. pgvector — the anticipated blocker, already resolved

This was flagged as the question to check first, since pgvector's own sqlx
compatibility is a hard external gate. Checked directly against pgvector's
published CHANGELOG
(`https://raw.githubusercontent.com/pgvector/pgvector-rust/master/CHANGELOG.md`):

```
## 0.4.2 (2026-05-22)
- Added support for SQLx 0.9
```

pgvector 0.4.2 was published **one day after** sqlx 0.9.0 (2026-05-21).
The workspace's pgvector pin is `pgvector = { version = "0.4", ... }`
(`Cargo.toml:330`, caret-equivalent) — 0.4.2 already satisfies that range,
so no pgvector version bump is even required, only a `cargo update` to
pick up the point release once the sqlx pins move. This is not a blocker.
It was the most likely one going in; it turned out to be the answer that
isn't a problem.

## 3. Blast radius — which crates touch sqlx, and how

11 crates reference `sqlx` somewhere in their `Cargo.toml` (dependency
declaration, weak-dependency forward, or a doc comment explaining a
deliberate *absence*): `cratestack-api`, `-cli`, `-client-rust`, `-client`,
`-macros`, `-migrate`, `-outbox`, `-pg`, `-service`, `-sqlx`, `-studio`.
Of those, only some actually compile against `sqlx-core`/`sqlx-postgres`
types:

| Crate | Relationship | Public-signature exposure |
|---|---|---|
| `cratestack-sqlx` | Direct `sqlx-core`/`sqlx-postgres` dep; owns the compatibility shim (§ below) | Yes — `Tx(sqlx::Transaction<'static, sqlx::Postgres>)` (`transaction.rs:56`), `ensure_migrations_table(pool: &sqlx::PgPool)` (`migrations.rs:55`) |
| `cratestack-outbox` | Direct dep on `cratestack-sqlx` (not sqlx itself) | Yes — `OutboxClient::from_pool(pool: sqlx::PgPool)` / `::pool(&self) -> &sqlx::PgPool` (`client.rs:27,33`) |
| `cratestack-macros` | No sqlx dep at all — but **generates code that references it** | Yes, widest radius: `include_server_schema!` codegen emits `pub fn builder(pool: ::cratestack::sqlx::PgPool) -> CratestackBuilder` and `pub fn pool(&self) -> &::cratestack::sqlx::PgPool` (`include/server/runtime/postgres.rs:51,61`) into **every** `db = Postgres` consumer's generated code |
| `cratestack-studio` | Direct `sqlx-core`/`sqlx-postgres` dep + `cratestack-sqlx` dep, for its Postgres data-browser | Internal only — no `pub` sqlx-typed signature found; but has the largest concentration of dynamic-SQL call sites (§4) |
| `cratestack-migrate` | Direct dep, gated behind an optional `postgres-introspect` feature | Internal only — introspection queries are all `&'static str` literals |
| `cratestack-cli` | Direct `sqlx-core`/`sqlx-postgres` dep, plus forwards `decimal-*` features to `cratestack-sqlx` | Internal/test only |
| `cratestack-pg` | Weak-dependency forward (`cratestack-sqlx?/...`) only, no direct sqlx dep | No sqlx types in its own signatures — re-exports `cratestack-sqlx`'s |
| `cratestack-service` | Same weak-dependency forward pattern | No |
| `cratestack-api`, `cratestack-client`, `cratestack-client-rust` | Structurally sqlx-free by design (`db = None` / client-only facades) — Cargo.toml comments there exist to document the *absence*, not a dependency | No |

The `PgPool` type itself (`sqlx_core::pool::Pool<Postgres>`) does not
change shape in 0.9.0 — nothing in the changelog touches `Pool<DB>`'s
public generic surface — so the widest-radius item (every generated
server's `CratestackBuilder::builder()`/`.pool()` signature) is a
recompile, not a rewrite, for downstream consumers. The real edits are
concentrated in `cratestack-sqlx` and `cratestack-studio` (§4).

### The direct-shim architecture — unaffected by the crate-splitting question

`crates/cratestack-sqlx/src/lib.rs:1-64` re-exports a `pub mod sqlx { ... }`
shim built entirely from `sqlx_core::*`/`sqlx_postgres::*` paths — the
mechanism `CLAUDE.md` and the module's own doc comment describe: going
direct to the split crates avoids `sqlx-sqlite` entering the resolve graph
and colliding with `rusqlite`'s own `libsqlite3-sys` via the `links =
"sqlite3"` rule. Nothing in the 0.9.0 changelog reorganizes the
`sqlx-core`/`sqlx-postgres`/`sqlx-sqlite`/`sqlx-mysql` crate split itself,
renames any of those crates, or changes how the umbrella `sqlx` crate
composes them (confirmed by diffing the umbrella crate's `[features]`
table at the `v0.9.0` tag against what this repo already expects — `postgres
= ["sqlx-postgres", "sqlx-macros?/postgres"]` is unchanged). The shim's own
doc comment already carries a caveat worth re-reading before the actual
upgrade PR: *"sqlx-core documents itself as 'not meant for general use'
without SemVer guarantees... stable in practice across 0.8.x. If sqlx-core
breaks at a 0.8 patch, this shim adapts in one place."* That sentence was
written with 0.8.x in mind; 0.9 is a minor bump specifically because the
project *does* consider it break-worthy (see §4) — the shim is still the
single adaptation point, which is the useful part of that design surviving
the version boundary, but the "stable in practice" claim shouldn't be
read as extending to 0.9 without verification.

## 4. The real work: `SqlSafeStr` and 12 call sites

The one 0.9.0 breaking change with concrete, non-hypothetical impact on
this codebase is PR #3723 ("Add SqlStr"):

> Breaking change: all `query*()` functions now take `impl SqlSafeStr`
> which is only implemented for `&'static str` and `AssertSqlSafe`. For
> all others, wrap in `AssertSqlSafe(<query>)`.

Verified directly against the `sqlx-core` source at the `v0.9.0` tag:
`sqlx_core::query::query()`/`query_with()` and `sqlx_core::raw_sql::raw_sql()`
now take `impl SqlSafeStr` (`sqlx-core/src/query.rs:653,668`), while
`QueryBuilder::new()`/`::push()` are **unaffected** — they still take
`impl Into<String>` / `impl Display` respectively
(`sqlx-core/src/query_builder.rs:56,122`). That distinction matters here
because this codebase's dynamic-filter machinery
(`push_filter_query`/`push_action_policy_query` in
`crates/cratestack-sqlx/src/query/support/`) is built entirely on
`QueryBuilder::push`, not on `query()` with a hand-formatted string — so
the bulk of the query-construction code is untouched.

What *is* touched: every call site that passes a non-`'static` string
directly into `query()`/`query_as()`/`raw_sql()`. Grepped for exhaustively
across every sqlx-touching crate (pattern: `query(&`/`raw_sql(&`/bare
`sql`-variable argument, cross-checked against source to rule out
`&'static str` literals):

- `crates/cratestack-sqlx/src/isolation.rs:60` — `sqlx::query(&set_stmt)`,
  `set_stmt` built via `format!("SET TRANSACTION ISOLATION LEVEL {}", ...)`
- `crates/cratestack-sqlx/src/delegate/view.rs:101` — `sqlx::query(&sql)`,
  `sql` built via `format!("REFRESH MATERIALIZED VIEW CONCURRENTLY {}", ...)`
- `crates/cratestack-sqlx/src/migrations.rs:141` — `sqlx::raw_sql(&migration.up)`,
  the migration file's own SQL text read at runtime
- `crates/cratestack-studio/src/data/postgres.rs:153`
- `crates/cratestack-studio/src/data/postgres/ops.rs:45,67,127`
- `crates/cratestack-studio/src/data/postgres/exec.rs:29,50,71`
- `crates/cratestack-studio/src/data/postgres/explain.rs:49`
- `crates/cratestack-cli/src/migrate/tests_baseline.rs:61` (test-only helper)

12 call sites, 4 files' worth of source (`cratestack-sqlx`: 3 sites in 3
files; `cratestack-studio`: 8 sites in 4 files — unsurprising, since
`cratestack-studio`'s data browser exists specifically to run
user-supplied/generated dynamic SQL against Postgres; `cratestack-cli`: 1
site, test-only). Every one of these needs the call site wrapped as
`sqlx::query(AssertSqlSafe(&sql))` (or the `raw_sql` equivalent) — a
mechanical, one-line-per-site change, not a redesign. `AssertSqlSafe` is
the escape hatch #3723 exists to provide for exactly this pattern
(building a query at runtime from trusted internal parts, as opposed to
directly formatting untrusted input into SQL); none of the 12 sites format
user-controlled string content into these particular strings (binds are
used for `$1`/`$2`-style values throughout — `set_stmt`/`sql`/`migration.up`
interpolate schema/table/isolation-level names, not row data), so this is
a mechanical satisfy-the-new-bound change, not a SQL-injection fix
disguised as one.

### Other 0.9.0 changes checked and ruled out as non-issues here

- **`Arguments` trait loses its lifetime parameter** (#3960, #3958,
  #3957): the `cratestack-sqlx` shim re-exports `Arguments`/`IntoArguments`
  raw (`lib.rs:19`) but this repo has no manual `impl Arguments for ...` —
  only `Encode`/`Decode`/`Type`/`PgHasArrayType` are manually implemented
  (for the local `Json<Value>` newtype, `crates/cratestack-sqlx/src/json.rs:77-108`),
  and none of those trait *signatures* change in 0.9.0. Low risk, but
  worth a real `cargo check -p cratestack-sqlx --all-features` in the
  implementation PR rather than assuming from this scan.
- **`#[derive(sqlx::Type)]` auto-generates `PgHasArrayType`** (#4008): no
  `derive(sqlx::Type)`/`derive(Type)` usage found anywhere in `crates/`
  (grepped explicitly) — this repo's only `Type`/`PgHasArrayType` impls
  are hand-written (`json.rs`), not derived, so there's no possible
  auto-derive/manual-impl conflict.
- **Postgres forces a generic query plan, may alter `query!()` macro
  output** (#3541): irrelevant — the workspace's `sqlx-core` feature list
  (`Cargo.toml:331`) does not include `macros`, so this repo never uses
  the compile-time `query!`/`query_as!` macros (which need a live/offline
  `DATABASE_URL` at compile time); all queries here are built dynamically
  through `QueryBuilder` or hand-written `&str`/`AssertSqlSafe`.
- **`PgConnectOptions::options()` auto-escaping** (#3800): no call site
  found using `.options()` in this codebase.
- **`sqlite`, `mysql`-specific breaking changes** (#3928, #3924, and the
  MySQL/SQLite items throughout "Breaking"): not applicable — this repo
  never enables `sqlx-sqlite`/`sqlx-mysql`; the embedded backend is
  `rusqlite` directly (see `CLAUDE.md`), which is the entire reason the
  direct-shim architecture in §3 exists.

## 5. MSRV — no conflict

sqlx 0.9.0's stated MSRV is **1.94.0** ("As per our MSRV policy, the
supported Rust version for this release cycle is 1.94.0" — CHANGELOG,
`## 0.9.0` → "Breaking"). This workspace's CI already runs an `msrv
(1.95.0)` job (`.github/workflows/ci.yml:58-59` on `origin/main`) and
declares `rust-version = "1.95.0"` in `[workspace.package]`
(`Cargo.toml:104` on `origin/main`) — one patch version *above* what sqlx
0.9.0 requires. Upgrading does not force an MSRV bump on this workspace.

## 6. Decimal backends and private features — unchanged

Checked directly against the `sqlx-core`/`sqlx-postgres` `Cargo.toml`
files at the `v0.9.0` git tag (not assumed from the changelog):

- `decimal-rust-decimal` / `decimal-bigdecimal` forward to
  `sqlx-core/rust_decimal`, `sqlx-postgres/rust_decimal`,
  `sqlx-core/bigdecimal`, `sqlx-postgres/bigdecimal`
  (`crates/cratestack-sqlx/Cargo.toml:26-27,48-49`) — all four feature
  names exist unchanged in the 0.9.0 `[features]` tables of both crates.
- The workspace's private/underscore sqlx-core features —
  `_rt-tokio` and `_tls-rustls-ring-webpki` (`Cargo.toml:331`) — both
  still exist, spelled identically, in `sqlx-core`'s 0.9.0 `[features]`
  table (`_rt-tokio = ["tokio", "tokio-stream"]`,
  `_tls-rustls-ring-webpki = ["_tls-rustls", "rustls/ring",
  "webpki-roots"]`). No renaming, no reorganization. This doesn't
  eliminate the risk the underscore convention itself signals (these
  remain explicitly unstable/SemVer-exempt by upstream's own naming
  convention — nothing prevents a *future* minor from moving them), it
  just means 0.9.0 specifically didn't act on that risk.
- `migrate`, `any`, `offline`, `chrono`, `uuid`, `json` — every other
  feature name this workspace pins on `sqlx-core`/`sqlx-postgres`
  (`Cargo.toml:331-332`) — are all present and unchanged in 0.9.0.

## 7. Estimate for an implementation ticket

Concrete, not adjectival:

- **2 files, 4 lines** — the lockstep version-pin bump: `Cargo.toml:331-332`
  (workspace root `sqlx-core`/`sqlx-postgres`) and
  `crates/cratestack-sqlx/Cargo.toml:92` (the `sqlx` resolver-anchor dep) —
  all three must move to the same new exact version together, or the
  duplicate-copy hazard in §1 reappears specifically for pgvector's
  `Encode`/`Type` impls.
- **1 dependency bump, no version pin change needed** — `pgvector = "0.4"`
  already admits 0.4.2; a `cargo update -p pgvector` picks up sqlx-0.9
  support once the pins move (§2).
- **12 call sites across 4 files, 3 crates** — wrap in `AssertSqlSafe(...)`
  per §4: `cratestack-sqlx/src/{isolation.rs,delegate/view.rs,migrations.rs}`
  (1 site each), `cratestack-studio/src/data/postgres/{postgres.rs,ops.rs
  (×3),exec.rs (×3),explain.rs}`, `cratestack-cli/src/migrate/tests_baseline.rs`
  (test-only).
- **0 public-signature rewrites** — `PgPool`'s shape is unchanged, so the
  widest-radius exposure (`cratestack-macros`' generated
  `builder()`/`.pool()` methods, §3) needs no codegen change, only a
  recompile.
- **Full workspace compile + the pgvector/decimal/postgres-introspect
  feature combinations** need exercising — this scan was static (grep +
  registry/changelog fetches), not a build; the actual PR must run `cargo
  check -p cratestack-sqlx --all-features`, `-p cratestack-studio
  --all-features`, `-p cratestack-migrate --features postgres-introspect`,
  and the PG-backed suite (`just test-pg`) to catch anything this
  investigation's static read missed (particularly any transitive-trait
  fallout from the `Arguments` lifetime removal noted in §4).
- **No MSRV follow-up needed** (§5), **no CI job changes anticipated**
  beyond the version bump itself.

This reads as a single PR of modest size — not a multi-PR migration —
once someone has a slot for it.
