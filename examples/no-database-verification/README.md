# no-database-verification

This crate is the story's headline piece of evidence (cratestack#329): a
real `cargo tree` proof that `sqlx`/`cratestack-sqlx` are absent from a
`db = None`-only consumer's dependency graph by default, and present when
the (default-on) `postgres` feature is enabled.

It is deliberately **not** a workspace member (see its entry in the parent
repo's root `Cargo.toml` `[workspace] exclude` list, and its own
`[workspace]` table). Cargo unifies a shared dependency's features across
every workspace member that builds it in the same session — so a
`default-features = false` example *inside* the workspace can still end up
with `sqlx` compiled in transitively, because some sibling member (a
`db = Postgres` example, or the crate's own test suite) wants the
`postgres` feature too, and Cargo builds one shared instance of
`cratestack-pg` for the whole session. Proving absence requires stepping
outside that shared graph entirely — a standalone crate with its own
`Cargo.lock`/target dir is the only way to see what an external consumer
building just this dependency tree would actually get.

## The schema

`schema.cstack` declares `datasource { provider = "none" }` and a single
`ping` procedure — the same shape as
`crates/cratestack-pg/tests/fixtures/no_database_procedures.cstack`
(cratestack#328's own proof fixture). `src/lib.rs` builds a real axum
router from it and `tests/smoke.rs` round-trips an HTTP call end to end,
so this isn't just a Cargo.toml assertion — the generated code actually
works with zero `sqlx` in the build.

## Run the proof yourself

```bash
cd examples/no-database-verification

# sqlx absent by default (postgres feature off)
cargo tree | grep -i sqlx   # -> no output

# sqlx present with the postgres feature on
cargo tree --features postgres | grep -i sqlx
# -> cratestack-sqlx v0.6.3 (...)
#    sqlx-core v0.8.6
#    sqlx-postgres v0.8.6
#    ...

# the router works either way
cargo test
cargo test --features postgres
```

## The mechanism

`crates/cratestack-pg/Cargo.toml` declares:

```toml
[features]
default = ["postgres", "decimal-rust-decimal", "codec-json"]
postgres = ["dep:cratestack-sqlx"]

[dependencies]
cratestack-sqlx = { workspace = true, optional = true }
```

`postgres` is default-on, so every existing `db = Postgres` consumer sees
zero change unless they opt out with `default-features = false` (this
crate's `Cargo.toml` does exactly that, then re-adds `postgres` behind its
own forwarding feature to produce the "present" half of the proof).
