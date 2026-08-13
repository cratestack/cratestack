# db-transaction-verification

This crate is the acceptance-bar proof for cratestack#513: a real
`Cargo.toml` and `cargo test` run you can point at showing that a service
can compose a multi-write Postgres transaction — `db.transaction(...)` —
using only CrateStack's own API, with **no `sqlx` entry in this crate's
`Cargo.toml`** and **no `sqlx::Transaction` (or any `sqlx::` path) named
anywhere in `src/lib.rs`**.

It is deliberately **not** a workspace member (see its entry in the parent
repo's root `Cargo.toml` `[workspace] exclude` list, and its own
`[workspace]` table) — same underlying reasoning as
`examples/no-database-verification` (cratestack#329): a real external
consumer's dependency graph is only visible from a crate with its own
`Cargo.lock`, not from an in-workspace member whose features Cargo unifies
with every sibling. The claim being proven here is slightly different,
though: `no-database-verification` proves `sqlx` is *transitively absent*
under `db = None`; this crate's schema uses `db = Postgres`, so `sqlx` **is**
present transitively (via `cratestack-sqlx`) — that's expected and correct.
What's being proven is narrower and exactly what cratestack#513 asks for:
this crate's own `Cargo.toml` never has to say so, and its own source never
has to name the type.

## The schema

`schema.cstack` declares two models, `Widget` and `WidgetNote`, so the
transaction under test is a genuine multi-model, multi-write transaction
rather than a single INSERT. `src/lib.rs`'s `create_widget_with_note`
writes both through `db.transaction(...)`:

```rust
db.transaction(async move |tx| {
    db.widget()
        .create(schema::CreateWidgetInput { id: widget_id, label })
        .run_in_tx(tx, ctx)
        .await?;

    db.widget_note()
        .create(schema::CreateWidgetNoteInput { id: note_id, widgetId: widget_id, note })
        .run_in_tx(tx, ctx)
        .await?;

    Ok(())
})
.await
```

Note what's absent: no `sqlx::Transaction` type annotation on `tx` (its
type is inferred), no `use sqlx::...` anywhere, and no `sqlx` line in
`Cargo.toml`.

## Run the proof yourself

```bash
cd examples/db-transaction-verification

# No direct sqlx dependency in this crate's own manifest:
grep -i '^sqlx' Cargo.toml   # -> no output

# sqlx IS present transitively (via cratestack-sqlx) — that's expected,
# not a bug; only the *direct* dependency and *named type* claims matter
# here:
cargo tree -i sqlx-core
# -> sqlx-core v0.8.6
#      cratestack-sqlx v... (path .../crates/cratestack-sqlx)
#        cratestack-pg v... (path .../crates/cratestack-pg)
#          db-transaction-verification v0.0.0 (this crate)

# No `sqlx::` path anywhere in this crate's own library source:
grep -rn 'sqlx' src/lib.rs   # -> only prose in doc comments, no `use`/path

# The transaction combinator actually works against a real Postgres
# (spins up an ephemeral testcontainer):
cargo test
```

`tests/transaction.rs` proves both directions against a real Postgres via
testcontainers: both writes commit together on success, and neither is
visible after the second write's failure rolls the transaction back. See
that file's own module doc comment for what it does and does not claim
about `sqlx` (it uses `cratestack::sqlx::query` for its own raw fixture
DDL/read-back assertions, the same way `crates/cratestack-pg/tests/
banking_*.rs` do — that's `cratestack`'s own re-export, not a direct `sqlx`
dependency, and it's orthogonal to the claim under test, which is about
`src/lib.rs`'s transaction call site).
