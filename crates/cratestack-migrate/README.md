# cratestack-migrate

Schema diff and migration generator for `.cstack` schemas. Produces SQL
migrations from the difference between a current `.cstack` and a
committed snapshot of the previously-generated schema.

This crate is the **authoring** side of the migration story. The
**runner** that applies SQL to a database lives in `cratestack-sqlx`
(forward-only, checksum-protected) and consumes the SQL produced here
identically to hand-written migrations.

See [ADR 0004](https://cratestack.dev/internals/schema-diff-adr) for the
full design.

## What it does

- Serializes a parsed `cratestack_core::Schema` into a committable
  snapshot (`schema.snapshot.json`).
- (Future slices) Diffs two snapshots into a backend-agnostic IR.
- (Future slices) Emits per-backend SQL — Postgres for sqlx targets,
  SQLite for rusqlite targets.
- (Future slices) Powers the `cratestack migrate diff` / `verify` /
  `drift` CLI commands.

## Current scope

The crate ships these surfaces:

- **Snapshot** (`Snapshot`, `read_snapshot`, `write_snapshot`,
  `read_or_empty`) — committable JSON form of a parsed `.cstack`.
- **Diff** (`diff`) — produces a backend-agnostic `Op` list given
  two parsed schemas.
- **IR** (`ir::Op`) — CreateTable / DropTable / Add+Drop Column /
  Add+Drop Index / AlterColumn (type/nullability/default) /
  Rename Table+Column / CreateEnum / AlterEnumAddVariant / DropEnum /
  AddCheck / DropCheck / CreateView / DropView / ReplaceView /
  CreateMaterializedView / DropMaterializedView.
- **Emitters** (`emit::postgres`, `emit::sqlite`) — render the IR
  to per-dialect SQL with up/down bodies, has_lossy /
  has_blocking flags, and explicit error stubs for destructive
  reversal.

Driven from the CLI by:

```
cratestack migrate diff \
  --schema schema.cstack \
  --out-dir migrations \
  --backend both \
  --name <slug> \
  [--allow-destructive]
```

### Not yet implemented

The full list of deferred work — across the migrate crate *and* the
rest of CrateStack — lives in the centralized [Roadmap]. The items
specific to this crate are:

- `cratestack migrate verify` — replay generated migrations against
  an ephemeral Postgres / SQLite and compare to the snapshot.
- `cratestack migrate drift` — introspect a live database and
  report differences from the snapshot.
- `DropEnumVariant` — needs the Postgres swap-dance and a backfill
  plan for referencing rows.

### View diff ordering

`view` blocks ([ADR 0003]) ship with the rest of the IR. The
diff engine projects views using the SQL body that matches the
schema's `datasource.provider` (`@@server_sql` on postgresql,
`@@embedded_sql` on sqlite), then interleaves view ops with the
rest of the migration:

- **View drops flush before column / table drops** — Postgres
  refuses to drop a column or table that still has a dependent
  view referencing it, so any view that touches a soon-to-be-
  dropped surface has to go first.
- **View creates flush after column / table creates** — source
  tables and any new columns the view body references have to
  exist before the view definition is parsed.

Body changes are modelled as `Drop + Create` rather than
`CREATE OR REPLACE VIEW` so the ordering works regardless of
which column ops the same migration includes. Within a Postgres
migration transaction other connections never observe the
transient missing-view state, so the atomicity loss has no
externally visible effect. The `ReplaceView` IR op is preserved
for hand-constructed callers; the diff engine no longer emits it.

Materialized views are server-only — the SQLite emitter treats
`CreateMaterializedView` / `DropMaterializedView` as `unreachable!`,
and the diff stage filters them out of SQLite projections so the
panic is defensive rather than reachable.

[ADR 0004]: https://cratestack.dev/internals/schema-diff-adr
[ADR 0003]: https://cratestack.dev/internals/views-adr
[Roadmap]: https://cratestack.dev/overview/roadmap
