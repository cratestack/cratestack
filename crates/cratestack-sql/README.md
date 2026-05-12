# cratestack-sql

Dialect-agnostic SQL primitives shared by `cratestack-sqlx` (Postgres) and `cratestack-rusqlite` (SQLite).

## Overview

`cratestack-sql` is the common type vocabulary the two backends agree on: column descriptors, filter and order ASTs, scalar value enums, and dialect placeholder logic.

Most users do not depend on this crate directly — it is reached transitively through `cratestack-sqlx` or `cratestack-rusqlite`. The types here surface in error messages and `descriptor()` accessors on generated delegates, so it is worth knowing the shapes.

## Installation

```toml
[dependencies]
cratestack-sql = "0.2.2"
```

## Scalar Values

`SqlValue` carries both typed values and per-type null markers (so a typed `NULL` round-trips correctly across the dialect):

```rust
pub enum SqlValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Uuid(uuid::Uuid),
    DateTime(chrono::DateTime<chrono::Utc>),
    Json(cratestack_core::Value),
    Decimal(cratestack_core::Decimal),
    NullBool, NullInt, NullFloat, NullString,
    NullBytes, NullUuid, NullDateTime, NullJson, NullDecimal,
}
```

`FilterValue` is `None | Single(SqlValue) | Many(Vec<SqlValue>)` for the IN / NOT IN operators.

The `IntoSqlValue` trait lifts Rust scalars into `SqlValue` automatically; generated `CreateModelInput` and `UpdateModelInput` impls compose it into `Vec<SqlColumnValue>` for INSERT / UPDATE.

## Dialect

The `Dialect` trait controls placeholder syntax. Two implementations exist:

- `PostgresDialect` — `$1`, `$2`, ...
- `SqliteDialect` — `?`, `?`, ...

## Filter AST

```rust
pub struct Filter {
    pub column: &'static str,
    pub op: FilterOp,
    pub value: FilterValue,
}

pub enum FilterExpr {
    Filter(Filter),
    All(Vec<FilterExpr>),
    Any(Vec<FilterExpr>),
    Not(Box<FilterExpr>),
    Relation(RelationFilter),
}
```

Generated field helpers (e.g. `cratestack_schema::post::published()`) return `FieldRef<M, T>` builders whose terminal methods (`eq`, `ne`, `gt`, `lt`, `is_true`, `like`, `starts_with`, ...) produce a `Filter`. Combine with `FilterExpr::all`/`any`/`not` or the fluent `.and(...)` / `.or(...)` combinators on `FilterExpr`.

`RelationFilter` carries the relation traversal metadata (`quantifier`, `parent_table`, `parent_column`, `related_table`, `related_column`) plus a boxed inner `FilterExpr`. `RelationQuantifier` is re-exported from `cratestack-policy`.

## Order AST

```rust
pub struct OrderClause { /* column + direction */ }
pub enum OrderTarget { /* scalar / relation / aggregate */ }
pub enum SortDirection { Asc, Desc }
```

`FieldRef::asc()` and `FieldRef::desc()` build the right `OrderClause`.

## Model Descriptor

`ModelDescriptor<M, PK>` is the metadata blob generated for each model. It captures table name, column list, primary key, allowed projection lists, policy slices (`read_allow_policies`, `read_deny_policies`, detail / create / update / delete pairs), default-value bindings, emitted-event kinds, optimistic-locking version column, audit flag, PII / sensitive column lists, and soft-delete retention metadata.

```rust
let desc = &cratestack_schema::USER_MODEL; // &'static ModelDescriptor<User, UserPk>
println!("table = {}", desc.table_name);
```

## See Also

- `cratestack-sqlx` — Postgres backend (async)
- `cratestack-rusqlite` — SQLite backend (sync, on-device)
- `cratestack-policy` — predicate enums embedded in `ModelDescriptor` policy slices

## License

MIT
