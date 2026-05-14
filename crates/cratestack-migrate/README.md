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
  AddCheck / DropCheck.
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

The following items from [ADR 0004] remain follow-up work:

- `cratestack migrate verify` — replay generated migrations against
  an ephemeral Postgres / SQLite and compare to the snapshot.
- `cratestack migrate drift` — introspect a live database and
  report differences from the snapshot.
- View IR ops (`CreateView` / `ReplaceView` / `DropView`,
  `CreateMaterializedView` / `DropMaterializedView`). The view
  block ([ADR 0003]) needs parser, AST, validator, and macro work
  before these ops have anything to consume.
- `DropEnumVariant` — needs the Postgres swap-dance and a backfill
  plan for referencing rows.

[ADR 0004]: https://cratestack.dev/internals/schema-diff-adr
[ADR 0003]: https://cratestack.dev/internals/views-adr
