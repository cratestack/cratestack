# declarative-query-verification

This crate is the acceptance-bar proof for cratestack#867: a real
`Cargo.toml` and `cargo test` run you can point at showing that a service
can run the two-aggregate `FILTER (WHERE …)` query epic cratestack#488 was
opened against — declared in `.cstack`, parameter-checked at compile time,
policy-gated at call time — with **no `sqlx` entry in this crate's
`Cargo.toml`**, **no `sqlx::` path in `src/`**, and **no SQL string in
`src/` either**.

That last one is the difference from its sibling
`examples/db-transaction-verification` (cratestack#513), which proves the
same "no direct `sqlx` dependency" claim for `db.transaction(...)`. Here
the SQL exists — it is the whole point — but it lives in `schema.cstack`,
where the framework can validate its `$N` references against the declared
parameters and attach a policy to it. Raw SQL in Rust gets neither.

It is deliberately **not** a workspace member (see its entry in the parent
repo's root `Cargo.toml` `[workspace] exclude` list, and its own
`[workspace]` table) — same underlying reasoning as
`examples/no-database-verification` (cratestack#329): a real external
consumer's dependency graph is only visible from a crate with its own
`Cargo.lock`, not from an in-workspace member whose features Cargo unifies
with every sibling. As with `db-transaction-verification`, `sqlx` **is**
present transitively here (via `cratestack-sqlx`) — that's expected and
correct; the claim is that this crate's own manifest and source never have
to say so.

## The schema

`schema.cstack` declares the query the epic was filed against:

```cstack
query loyaltyFeeSummary(userId: String, cutoff: DateTime): LoyaltyFeeSummary
  @@sql("""
    SELECT
      COALESCE(SUM(discount), 0)::bigint AS "total",
      COALESCE(SUM(discount) FILTER (WHERE created_at >= $2), 0)::bigint AS "thisMonth"
    FROM loyalty_fee_events
    WHERE user_id = $1
  """)
  @allow(auth() != null && auth().subjectId == userId)
```

Three things this buys over `db.pool()` + `sqlx::query_as`:

1. **The parameters are checked when you build.** Typing `$3` where you
   meant `$2` fails `cargo check` with a message naming the query and the
   declared parameter count — and so does leaving a declared parameter
   unreferenced, which is the half of that typo a one-directional check
   would miss.
2. **The policy is not optional.** `@allow` is evaluated inside the single
   generated entry point, before any SQL runs. There is no unchecked
   variant to reach for, and a query that declares no `@allow` denies
   everyone.
3. **The result is a declared type**, decoded into real Rust fields rather
   than a `PgRow` the caller `try_get`s out of by hand.

### It reads only

The generated entry point runs the statement inside a Postgres `READ ONLY`
transaction, so `INSERT`/`UPDATE`/`DELETE`/`TRUNCATE` and DDL are refused
by the engine (SQLSTATE `25006`) — including when hidden inside a
data-modifying CTE like `WITH ins AS (INSERT … RETURNING …) SELECT …`,
which is an ordinary `SELECT` as far as the driver is concerned.

That matters because a write reaching the database this way would bypass
`@@audit` rows, the `@@emit` outbox, `@version` optimistic locking,
soft-delete, `@@internal` suppression and the target model's own write
`@@allow`. Use a `procedure` or a model write builder to change data.

### It runs on its own connection

A query takes a connection from the pool, so it does **not** observe
uncommitted writes made by an enclosing `db.transaction(...)`. Read after
that transaction commits.

That connection is a *second* one for as long as the query runs. Calling a
query from inside a transaction on a pool with no free slot does not
return a stale row — it blocks for `acquire_timeout` and then fails with
"pool timed out while waiting for an open connection". On a small pool
that is a deadlock, not a surprise about isolation.

### What it does *not* buy

`@allow` gates **whether the call is permitted**, not **which rows the SQL
matches**. Nothing injects a `deleted_at IS NULL` predicate or a row-level
policy filter into a `query` body the way the generated read path does —
you own every `WHERE`/`FILTER` predicate here. See
`docs/design/declarative-custom-query.md` §6.

Note also the quoted `AS "thisMonth"`. A query's `SELECT` list is yours,
so nothing inserts an alias for you, and Postgres folds unquoted
identifiers to lower case — the row decoder looks up the declared field
name exactly.

## Run the proof yourself

```bash
cd examples/declarative-query-verification

# No direct sqlx dependency in this crate's own manifest:
grep -iE '^\s*sqlx\s*=' Cargo.toml   # -> no output

# No `sqlx::` path and no SQL anywhere in this crate's own library source:
grep -rn 'sqlx\|SELECT' src/         # -> only prose in doc comments

# sqlx IS present transitively (via cratestack-sqlx) — expected, not a bug:
cargo tree -i sqlx-core

# The query actually runs against a real Postgres (ephemeral testcontainer).
# DOCKER_HOST is derived rather than hardcoded — the value encodes a uid,
# and testcontainers-rs does not read `docker context`, so on rootless
# Docker the container never starts and the skip reports as a pass:
export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
export CRATESTACK_REQUIRE_DB=1
cargo test
```

`tests/query.rs` proves both directions against a real Postgres: an
admitted principal gets the correct two aggregates (`total` 380,
`thisMonth` 280 — so a `FILTER` clause that stopped applying would fail
here, not pass quietly), and a principal the `@allow` does not admit gets
`Forbidden`. See that file's own module doc for what it does and does not
claim about `sqlx` — it uses `cratestack::sqlx::query` for its own fixture
DDL and seeding, the same way `crates/cratestack-pg/tests/banking_*.rs`
do, which is orthogonal to the claim under test.
