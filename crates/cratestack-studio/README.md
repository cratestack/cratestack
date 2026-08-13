# cratestack-studio

Embedded admin and testing surface for `.cstack` schemas: a single binary
that loads one or more schemas described in a `studio.toml` workspace
file, opens the configured database and/or service connections, and
serves a JSON API plus a bundled web UI for browsing and editing data.

This crate replaces the old per-project `cratestack-studio-generator`
scaffold, which was a thin re-export of this crate's `eject` function and
has since been removed (#427) now that nothing in the workspace depended
on it directly — see [Eject](#eject) below.

## Quickstart

```bash
cratestack studio init           # writes ./studio.toml
cratestack studio run            # binds 127.0.0.1:7878 by default
```

`studio init` writes a heavily commented starter `studio.toml`; fill in
at least one `[[target]]` with a `schema` path and a `[target.db]` and/or
`[target.api]` block. `studio run` loads the workspace, opens the
configured connections, and serves the API and UI at the bind address
(`--bind` overrides the default).

## What it does

- **Read API** — list/paginate/get records, follow relations, fetch a
  typed `find_unique` Rust snippet for a row, all across every
  configured target (direct Postgres/SQLite connection, or proxied
  through a deployed CrateStack service).
- **Write API** — `POST`/`PATCH`/`DELETE` on `rw`-mode targets, with the
  request payload validated against the model's schema attributes
  (`@email`, `@length`, `@range`, `@regex`, `@uri`, `@iso4217`, plus
  implicit required/type checks) before any SQL runs. Driver-level
  constraint failures (unique, foreign key, not-null, check, string
  truncation) are mapped back to the same `422 VALIDATION_ERROR`
  envelope. `ro`-mode targets reject writes with `403 FORBIDDEN`.

  **`[target.db]` is a direct SQL connection, not the generated API** —
  see the crate's rustdoc for the full rationale (cratestack#507, #553).
  `@version` bumping is routed for real on every `[target.db]` backend
  (Postgres and SQLite alike), so a `@version`-only model is never
  refused. `@@emit(...)` is routed for real — a `cratestack_event_outbox`
  row lands in the same transaction — **only on Postgres**; SQLite has
  no event-outbox equivalent, and `include_embedded_schema!` treats
  `@@emit(...)` as a no-op on the framework's own embedded backend, so
  this is a permanent backend capability difference, not an unfinished
  feature. Studio refuses `POST`/`PATCH`/`DELETE` against an
  `@@emit(...)` model on a non-Postgres `[target.db]` target with a
  `403 UNSAFE_DB_WRITE` naming the attribute, unless that target sets
  `allow_unsafe_writes = true`. `[target.api]` targets are unaffected —
  those writes go through the deployed service's own generated routes,
  which already apply `@version`/`@@emit`/`@@allow`. `@@allow`
  enforcement on `[target.db]` itself is unchanged by this — it remains
  unenforced on both reads and writes there, which is worth weighing
  before pointing Studio at a schema with policy-gated or `@sensitive`
  fields.
- **SQL preview + query plans** — render the SQL an operation would run
  without touching the database, optionally asking the driver to
  `EXPLAIN` it. Studio never issues `EXPLAIN ANALYZE`.
- **Drift detection** — compares a schema's declared columns against the
  live database and reports per model (`ok` / `drift` / `missing_table`
  / `unsupported` / `skipped`).
- **CSV/JSON export** and **schema search** (models, fields, enums,
  mixins, types, procedures) round out the admin surface.
- **Audit log** — every successful write is recorded in an in-memory
  ring buffer (capped, FIFO) served at `GET /api/audit`. Set
  `[workspace] audit_file` in `studio.toml` to also persist entries to an
  append-only JSONL file that's replayed on boot; left unset (the
  default), Studio writes nothing to disk.

See the [Studio docs](https://cratestack.dev/studio/quickstart) for the
full `studio.toml` reference, endpoint list, and error codes.

## UI

Studio ships a Leptos+Trunk browser UI (target switcher, records table,
typed create/edit forms, relation picker, SQL/EXPLAIN preview, drift
indicators, schema search, audit overlay). It's bundled into this
crate's binary via the `embed-ui` cargo feature, which is **on by
default**. When there's no Trunk build to embed (e.g. a plain checkout
build without running Trunk first), the server falls back to a
placeholder page at `/` instead of failing the build — the JSON API
under `/api/*` is unaffected either way.

To hack on the UI itself, run `cratestack studio run` in one terminal
and `trunk serve` (in the sibling `cratestack-studio-ui` crate) in
another.

## Eject

`cratestack studio eject --out <dir>` scaffolds a standalone, runnable
binary crate that depends on the published `cratestack-studio` crate
(`Cargo.toml`, `studio.toml`, an example schema, `src/main.rs`) — the UI
is already bundled in, no Trunk/wasm toolchain required. Pass `--with-ui`
to additionally unpack the Leptos+Trunk UI source tree into `<out>/ui/`
for front-end customization (`trunk serve` there against the ejected
binary). Pass `--force` to overwrite a non-empty output directory.

## License

MIT
