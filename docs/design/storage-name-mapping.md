# `@@map` / `@map`: storage-only table and column names (cratestack#1375)

Status: **implemented** (0.20.x). Additive: a schema without the markers
generates, migrates and runs exactly as before.

A schema often has to sit on a database that already exists, with a table
called `legacy_accounts` or a column called `ssn_enc`. Until now the table was
always `pluralize(snake_case(Model))` and the column `snake_case(field)`. This
adds two attributes that change the name SQL uses and nothing else.

```cstack
model Account {
  id Int @id @map("account_id")
  ssn String @pii @map("ssn_enc")
  displayName String @map("display_name")

  @@map("legacy_accounts")
}
```

## 1. Decisions

1. **Storage only.** `@@map("<table>")` goes on a model; `@map("<column>")` on
   a stored scalar field of a model or mixin. Both are refused where they would
   rename nothing: on a view (`@@map`), and on a relation field, a
   `@computed` field, a `type` field, an `auth` field or a to-many back
   reference (`@map`). Enum values are out of scope.
2. **Routes do not move.** The REST path (`model_route_segment`), the RPC op id
   (`model.<Model>.<verb>`), client accessors, generated identifiers, wire field
   names, sort and cursor keys all come from the model and field names. No
   client generator, and no `axum`/`rpc` dispatch code, reads the mapping.
3. **Contract.** Both attributes are on `DROPPED_ATTRIBUTES` ("storage: SQL
   table/column name"): op digests and contract locks do not change, and
   `cratestack diff` reports a changed `@@map` as `Internal`, never `Breaking`.
4. **Name grammar.** Exactly one double-quoted argument matching
   `^[a-z_][a-z0-9_]*$`, at most 63 bytes. No `.` (so no schema qualifier), no
   quoting. Not a PostgreSQL or SQLite reserved word. A table may not start with
   `cratestack_`, `sqlite_` or `pg_`. At most one marker per model or field.
   Generated SQL does not quote identifiers, so `@@map("user")` would read
   `FROM user` (Postgres's `current_user`) and `@@map("order")` would not parse:
   reserved words are refused until identifiers are quoted (a follow-up).
5. **Uniqueness.** Final table names are unique across models and views. Final
   column names are unique among a model's *stored* fields (not relation or
   `@computed` fields, so `ownerId Int @map("owner")` may sit beside
   `owner User`), counting the implicit `deleted_at` of `@@soft_delete`: a
   field may not be mapped to it, nor `deletedAt` mapped away from it. The
   generated-identifier check (`to_snake_case` of every field name, so
   `fooBar` beside `foo_bar` is refused even when one is mapped) is unchanged. `@@map("deposits")` beside a
   `Deposit` model is a collision.
6. **Migrate.** See section 3.
7. **Hand-written SQL** (a view's `@@server_sql` / `@@sql`, `query` blocks,
   `up.pre.sql`) must use the mapped names.
8. **Errors.** The API conflict message is unchanged. The studio's
   column-to-field lookup for a database error uses the mapped names.

## 2. One reader

`cratestack_core::schema::storage_names` holds the grammar and the readers:
`Model::sql_table_name`, `Model::sql_column_of(field)` and
`Field::sql_column_name`. The defaults are the old derivation, so an unmapped
schema is unchanged. The parser validates every marker with the reader the
generators use (so a marker that parsed is a marker that is read), and the
collision checks compare final names.

Everything that builds SQL asks the reader: the macros (descriptor, columns,
unique/default/generated columns, row decoders, field modules, relation links,
order catalogs, policy predicates, subscription feeds, a view's source tables),
migrate, and the studio. The runtime needs no change for plain reads and
writes: `ModelDescriptor` already carries `ModelColumn { rust_name, sql_name }`
and reads `sql_name AS "rust_name"`.

A tripwire (`client_contract/tests_readers_storage.rs`) pins which source files
call each helper and fails if a route, wire or client file ever does.

### The audit fix

The descriptor holds SQL names (`pii_columns`, `sensitive_columns`,
`primary_key`) but an audit snapshot is the serialized model, keyed by field
name. Redaction bridged the two with a `snake_to_camel` guess, which holds only
while a column is the snake_case of its field. With `@pii @map("ssn_enc")` the
value was logged in clear, and a mapped `@id` was recorded as a null primary
key. `ModelDescriptor::rust_name_of` translates before redaction and before the
primary-key lookup.

## 3. Migrate

Tables and columns are matched by name, so a changed `@@map` or `@map` would be
read as drop-and-create: the data gone, an empty table in its place. It is
refused instead.

- `TableProjection` gains `model` and `field_columns` (serde-defaulted and
  skipped when empty, so an introspected projection, a `migrate baseline`
  snapshot and its checksum are unchanged). They say which model a table stores
  and which column each field lives in.
- `diff_projections` runs `check_storage_names_stable` before computing ops. A
  table whose model was stored under another name, or a field whose column
  changed, is `TableStorageNameChanged` / `ColumnStorageNameChanged`, naming the
  marker to add: `@@rename(from = "<old table>")` or
  `@rename(from = "<old column>")`. With it, the change is one `RenameTable` /
  `RenameColumn`.
- A snapshot written before these fields existed has none; it is read with
  every table and column under its derived name.
- A model renamed while keeping its `@@map`, and a new mapped field, are not
  changes.
- **Adopting an existing database:** write the `@@map` / `@map`, then run
  `cratestack migrate baseline` against it, so the snapshot starts from the
  database as it is.
- Index, unique-constraint and foreign-key names derive from the mapped table
  and column names, so they can differ from the names an existing database
  already has: the first `migrate diff` after a baseline may drop and re-add
  indexes.
- A deliberate drop-and-recreate of a table under another name is refused too;
  the way is two migrations.
- New snapshots carry `model` / `field_columns`: a one-time textual diff.
- **Baseline stamps the snapshot.** An introspected table has no model, so
  `migrate baseline` copies `model` and the field-to-column map from the
  authored schema onto every table that has the same name (a column only if
  the database has it). The baseline checksum is of the introspected shape,
  unstamped, so it is unchanged. Where the snapshot still records nothing (a
  baseline whose table name differed from the schema's, or a snapshot from
  before the fields), a mapped model or column that is new while an
  unrecorded table or column vanishes is refused
  (`TableStorageNameUnrecorded` / `ColumnStorageNameUnrecorded`); a
  deliberate drop and create is two migrations. The baseline's own drift
  report uses `diff_for_drift`, which does not refuse names.
- Identity metadata is not DDL: comparing an introspected table with an
  authored one uses `TableProjection::without_storage_identity`.
- A no-op `migrate diff` does not rewrite the snapshot, so an older snapshot
  stays unstamped until the next real change; the derived-name fallback covers
  it, and a baseline snapshot is stamped when written.
- **Embedded (`include_embedded_schema!`)** has no migrate step. Adding or
  changing `@@map` on a device creates a new empty table and orphans the old
  rows; adding `@map` to an existing column errors. Re-sync or wipe an
  embedded cache when a mapping changes.

## 4. Verification

- Parser: forms, grammar, reserved words, placements and every collision
  (`tests_storage_map*.rs`).
- Postgres, over hand-written DDL under the mapped names: every ORM operation,
  audit redaction and primary key, REST routes and RPC ops
  (`crates/cratestack-pg/tests/storage_map*.rs`).
- SQLite: the DDL and the operations (`crates/cratestack-sqlite/tests/storage_map.rs`).
- Migrate: projection, refusals, rename escape, old snapshots, round trip, and
  the emitted DDL.
- Parity: the generated TypeScript, Dart and WireMock output is byte-identical
  with and without the markers, under REST and RPC.

## 5. Out of scope

Quoting identifiers (which would admit reserved words and mixed case), a
schema qualifier (`@@map("audit.events")`), `@map` on enum values, and mapping
a view's own name.
