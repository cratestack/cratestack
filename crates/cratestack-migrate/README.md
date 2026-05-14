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

Slice 1 — snapshot serialization only. The diff engine, emitters, and
CLI commands land in subsequent slices.
