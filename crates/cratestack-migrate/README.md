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

- Projects a parsed `cratestack_core::Schema` (or, for Postgres, a live
  database via `introspect::postgres`) into `Projections` — a
  backend-agnostic IR of every table/view's SQL shape — and serializes
  it into a committable snapshot (`schema.snapshot.json`).
- Diffs two `Projections` values (`diff_projections`) into a
  backend-agnostic `Op` list. `diff(prev: &Schema, next: &Schema)` is a
  thin `Schema → Schema` wrapper around it for callers that only ever
  have two parsed schemas on hand.
- Emits per-backend SQL — Postgres for sqlx targets, SQLite for
  rusqlite targets.
- Powers the `cratestack migrate diff` and `cratestack migrate
  baseline` CLI commands. `verify` is not yet implemented — see below.

## Current scope

The crate ships these surfaces:

- **Projection** (`project`, `Projections`) — the `Schema → IR` seam:
  lowers a parsed `.cstack` schema into the same `Projections` shape
  live introspection produces, so both sides of a diff can come from
  either source.
- **Snapshot** (`Snapshot`, `read_snapshot`, `write_snapshot`,
  `read_or_empty`) — committable JSON form of a `Projections` value
  (format version 2 — earlier versions stored a full `Schema` and are
  rejected; see `snapshot.rs`'s module doc for the rationale). This is
  what `cratestack migrate diff` reads/writes as the previous-state
  file, and what `cratestack migrate baseline` writes from an
  introspected live database.
- **Diff** (`diff`, `diff_projections`) — produces a backend-agnostic
  `Op` list from two schemas or two `Projections` values directly.
- **Introspection** (`introspect::postgres`, behind the
  `postgres-introspect` feature) — produces a `Projections` value from
  a live `sqlx_postgres::PgPool` instead of a parsed schema. Powers
  `cratestack migrate baseline` (issue #205); see the module's own doc
  comment for the known gaps (no foreign-key introspection,
  multi-/zero-column CHECK constraints skipped, etc.).
- **Checksum** (`projections_checksum`) — SHA-256 fingerprint of a
  `Projections` value, used by `migrate baseline`'s synthetic
  `cratestack_migrations` row.
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

cratestack migrate baseline \
  --schema schema.cstack \
  --database-url postgres://... \
  --out-dir migrations \
  [--strict]
```

### Not yet implemented

The full list of deferred work — across the migrate crate *and* the
rest of CrateStack — lives in the centralized [Roadmap]. The items
specific to this crate are:

- `cratestack migrate verify` — replay generated migrations against
  an ephemeral Postgres / SQLite and compare to the snapshot.
- `DropEnumVariant` — needs the Postgres swap-dance and a backfill
  plan for referencing rows.
- SQLite/embedded baselining — `migrate baseline` is Postgres-only for
  v1 (design doc `docs/design/migrate-baseline.md` §6).

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
