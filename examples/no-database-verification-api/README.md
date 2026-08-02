# no-database-verification-api

This crate is cratestack#347's headline piece of evidence: a real `cargo
tree` proof that `sqlx`/`cratestack-sqlx` are absent from a `cratestack-api`
consumer's dependency graph.

It is deliberately **not** a workspace member (see its entry in the parent
repo's root `Cargo.toml` `[workspace] exclude` list, and its own
`[workspace]` table) — same reasoning as
[`examples/no-database-verification`](../no-database-verification)
(cratestack#329): Cargo unifies a shared dependency's features across every
workspace member that builds it in the same session, so proving absence
requires stepping outside that shared graph entirely.

Unlike that crate, there's no `postgres`-style feature to toggle here for a
"present" half of the proof: `cratestack-api`'s `Cargo.toml` never lists
`cratestack-sqlx` as a dependency, under any feature, so there is exactly
one state to demonstrate.

## The schema

`schema.cstack` declares `datasource { provider = "none" }` and a single
`ping` procedure — the same shape as
`crates/cratestack-api/tests/fixtures/no_database_procedures.cstack` and
`examples/no-database-verification/schema.cstack`. `src/lib.rs` builds a
real axum router from it and `tests/smoke.rs` round-trips an HTTP call end
to end, so this isn't just a `Cargo.toml` assertion — the generated code
actually works with zero `sqlx` in the build.

## Run the proof yourself

```bash
cd examples/no-database-verification-api

# sqlx absent — cratestack-api never depends on it, no feature required
cargo tree | grep -i sqlx   # -> no output

# the router works
cargo test
```

## The mechanism

`crates/cratestack-api/Cargo.toml` simply has no `cratestack-sqlx` entry in
`[dependencies]` — compare `crates/cratestack-pg/Cargo.toml`, where
`cratestack-sqlx` is `optional = true` behind a default-on `postgres`
feature. There's nothing to opt out of here: this facade only ever exists
for `db = None` schemas, which can never declare a `model`
([cratestack#327](https://github.com/cratestack/cratestack/issues/327)), so
there's no code path that would ever need the sqlx-backed symbols in the
first place.

`include_server_schema!(schema, db = Postgres)` under this dependency fails
to compile with a clear error rather than silently doing the wrong thing —
see [`crates/cratestack-api/README.md`](../../crates/cratestack-api/README.md)
for that reproduction.
