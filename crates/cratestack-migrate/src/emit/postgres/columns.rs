//! Column-level DDL: ADD / DROP / RENAME / ALTER (type, nullability,
//! default), plus the `render_column` / `render_type` helpers that
//! [`super::tables::emit_create_table`] also leans on.

use std::fmt::Write as _;

use crate::ir::{
    AddColumn, AlterColumnDefault, AlterColumnNullability, AlterColumnType, Column, ColumnArity,
    ColumnDefault, ColumnType, DropColumn, RenameColumn,
};
use crate::naming;

use super::idents::quote_ident;

pub(super) fn emit_add_column(sql: &mut String, add: &AddColumn) {
    writeln!(
        sql,
        "ALTER TABLE {} ADD COLUMN {};",
        quote_ident(&add.table),
        render_column(&add.column)
    )
    .unwrap();
}

pub(super) fn emit_drop_column(sql: &mut String, drop: &DropColumn) {
    writeln!(
        sql,
        "ALTER TABLE {} DROP COLUMN {};",
        quote_ident(&drop.table),
        quote_ident(&drop.column)
    )
    .unwrap();
}

pub(super) fn emit_rename_column(sql: &mut String, rename: &RenameColumn) {
    writeln!(
        sql,
        "ALTER TABLE {} RENAME COLUMN {} TO {};",
        quote_ident(&rename.table),
        quote_ident(&rename.from),
        quote_ident(&rename.to)
    )
    .unwrap();
}

pub(super) fn emit_alter_column_type(sql: &mut String, alter: &AlterColumnType) {
    let rendered = render_type(&alter.to, alter.to_arity);
    writeln!(
        sql,
        "ALTER TABLE {} ALTER COLUMN {} TYPE {} USING ({}::{});",
        quote_ident(&alter.table),
        quote_ident(&alter.column),
        rendered,
        quote_ident(&alter.column),
        rendered
    )
    .unwrap();
}

pub(super) fn emit_alter_column_nullability(sql: &mut String, alter: &AlterColumnNullability) {
    match (alter.from, alter.to) {
        (ColumnArity::Required, ColumnArity::Optional) => writeln!(
            sql,
            "ALTER TABLE {} ALTER COLUMN {} DROP NOT NULL;",
            quote_ident(&alter.table),
            quote_ident(&alter.column)
        )
        .unwrap(),
        (ColumnArity::Optional, ColumnArity::Required) => writeln!(
            sql,
            "ALTER TABLE {} ALTER COLUMN {} SET NOT NULL;",
            quote_ident(&alter.table),
            quote_ident(&alter.column)
        )
        .unwrap(),
        // List ↔ scalar flips reshape data and ride along with
        // AlterColumnType — no standalone nullability statement.
        _ => {}
    }
}

pub(super) fn emit_alter_column_default(sql: &mut String, alter: &AlterColumnDefault) {
    match &alter.to {
        Some(ColumnDefault::Literal(value)) => emit_set_default(sql, alter, value),
        Some(ColumnDefault::Function(call)) => emit_set_default(sql, alter, call),
        // `dbgenerated()` never has DDL to set — dropping any
        // previously-managed default hands the column back to
        // whatever external mechanism is expected to supply it.
        Some(ColumnDefault::DbGenerated) | None => writeln!(
            sql,
            "ALTER TABLE {} ALTER COLUMN {} DROP DEFAULT;",
            quote_ident(&alter.table),
            quote_ident(&alter.column)
        )
        .unwrap(),
    }
}

fn emit_set_default(sql: &mut String, alter: &AlterColumnDefault, rendered: &str) {
    writeln!(
        sql,
        "ALTER TABLE {} ALTER COLUMN {} SET DEFAULT {};",
        quote_ident(&alter.table),
        quote_ident(&alter.column),
        rendered
    )
    .unwrap();
}

pub(super) fn render_column(column: &Column) -> String {
    let mut buf = quote_ident(&column.name);
    buf.push(' ');
    buf.push_str(&render_type(&column.ty, column.arity));
    if matches!(column.arity, ColumnArity::Required | ColumnArity::List) {
        buf.push_str(" NOT NULL");
    }
    match &column.default {
        Some(ColumnDefault::Literal(value)) => {
            buf.push_str(" DEFAULT ");
            buf.push_str(value);
        }
        Some(ColumnDefault::Function(call)) => {
            buf.push_str(" DEFAULT ");
            buf.push_str(call);
        }
        // No DDL default for `dbgenerated()` — see `ColumnDefault::DbGenerated`.
        Some(ColumnDefault::DbGenerated) | None => {}
    }
    buf
}

fn render_type(ty: &ColumnType, arity: ColumnArity) -> String {
    let base = match ty {
        ColumnType::Scalar(name) => scalar_to_postgres(name).to_owned(),
        // Enums are stored as TEXT, not as a native `CREATE TYPE ...
        // AS ENUM`. The generated row decoders read every enum field
        // with `try_get::<String>` and `.parse()`, so a native enum
        // column fails to decode on every read (issue #228). The
        // validation the native type would have given is recovered by
        // a `CHECK (col IN (...))` constraint — see
        // `super::checks` and `crate::convert::enum_check_kind`.
        ColumnType::Enum(_) => "TEXT".to_owned(),
        // Composite type identifiers are snake-cased so the SQL type
        // name matches the convention used elsewhere in the generator
        // (tables, columns) and so that case-mismatched references
        // don't silently resolve to different identifiers under
        // Postgres's unquoted-lowercase rule.
        ColumnType::UserDefined(name) => quote_ident(&naming::column_name(name)),
        ColumnType::Vector(dimension) => render_vector_type(*dimension),
        ColumnType::Spatial {
            geography,
            subtype,
            srid,
        } => render_spatial_type(*geography, subtype.as_deref(), *srid),
    };
    match arity {
        ColumnArity::List => format!("{base}[]"),
        _ => base,
    }
}

/// Renders `Vector(n)` as Postgres's parametric `vector(n)` column
/// type (the `pgvector` extension — see `docs/design/extensions.md`
/// §6). Gated behind the `pgvector` Cargo feature: reaching this with
/// the feature disabled means an `Op::EnsureExtension`/`ColumnType::
/// Vector` was constructed without going through the parser's own
/// gate (`extension pgvector { }` must be declared for `Vector(n)` to
/// parse at all), so a hard panic is the right failure mode rather
/// than silently emitting a `vector(n)` column type this build never
/// opted into supporting.
#[cfg(feature = "pgvector")]
fn render_vector_type(dimension: u32) -> String {
    format!("vector({dimension})")
}

#[cfg(not(feature = "pgvector"))]
fn render_vector_type(dimension: u32) -> String {
    unreachable!(
        "ColumnType::Vector({dimension}) reached the Postgres emitter without the \
         `pgvector` Cargo feature enabled on cratestack-migrate — this should be \
         unreachable because only a schema declaring `extension pgvector {{ }}` produces a \
         `Vector(n)` column, and cratestack-parser requires that declaration up front"
    );
}

/// Renders a `Geography`/`Geometry` field as its PostGIS column type
/// (cratestack#842). Gated behind the `postgis` Cargo feature for the
/// same reason as [`render_vector_type`]: reaching this without the
/// feature means a `ColumnType::Spatial` was constructed without going
/// through the parser's own `extension postgis { }` gate.
///
/// The modifier is positional and rendered without a space
/// (`geography(Polygon,4326)`), matching how PostGIS itself formats the
/// type in `information_schema` — so a later introspection diff of the
/// same column compares equal instead of reporting a phantom change.
#[cfg(feature = "postgis")]
fn render_spatial_type(geography: bool, subtype: Option<&str>, srid: Option<u32>) -> String {
    let base = if geography { "geography" } else { "geometry" };
    match (subtype, srid) {
        (Some(subtype), Some(srid)) => format!("{base}({subtype},{srid})"),
        (Some(subtype), None) => format!("{base}({subtype})"),
        // An SRID with no subtype is rejected at parse time (PostGIS's
        // modifier is positional), so this collapses to the bare type.
        (None, _) => base.to_owned(),
    }
}

#[cfg(not(feature = "postgis"))]
fn render_spatial_type(geography: bool, _subtype: Option<&str>, _srid: Option<u32>) -> String {
    let written = if geography { "Geography" } else { "Geometry" };
    unreachable!(
        "ColumnType::Spatial reached the Postgres emitter without the `postgis` Cargo feature \
         enabled on cratestack-migrate — this should be unreachable because only a schema \
         declaring `extension postgis {{ }}` produces a `{written}` column, and cratestack-parser \
         requires that declaration up front"
    );
}

/// Maps a `.cstack` builtin scalar name to its Postgres column type.
///
/// `name` is only ever one of `cratestack_parser::builtin_type_names()`
/// (minus `Page`, which never reaches here — see `convert/fields.rs`'s
/// `ColumnType::Scalar` vs `Enum`/`UserDefined` split) or an unrecognized
/// name from a schema this crate doesn't validate directly. There is
/// deliberately no `"Date"` arm: `BUILTIN_TYPES` has never contained a
/// bare `Date` type (only `DateTime`), so that arm was unreachable dead
/// code that read as a supported feature — see cratestack#232. New
/// builtins need a matching arm added here.
fn scalar_to_postgres(name: &str) -> &'static str {
    match name {
        "String" | "Cuid" => "TEXT",
        "Int" => "BIGINT",
        "Float" => "DOUBLE PRECISION",
        "Decimal" => "NUMERIC",
        "Boolean" => "BOOLEAN",
        "DateTime" => "TIMESTAMPTZ",
        "Json" => "JSONB",
        "Bytes" => "BYTEA",
        "Uuid" => "UUID",
        // Unknown scalars fall back to TEXT. Note this arm discards
        // `name` rather than passing it through — an earlier version of
        // this comment claimed the opposite ("passed through unquoted —
        // the developer is responsible"), which is not what the code
        // does and cannot be: `-> &'static str` can't return a borrowed
        // `name`. That wrong comment was read as documenting a real
        // escape hatch and led to cratestack#842 being filed against
        // behaviour this function doesn't have.
        //
        // The fallback is *not* reachable from a `.cstack` file: every
        // entry point that parses one (`parse_schema`,
        // `parse_schema_file`, and therefore `cratestack migrate diff`)
        // runs `validate_type_ref`, which rejects any name outside
        // `cratestack_parser::builtin_type_names()` with "unknown type
        // `X`". It survives only for `Schema` values this crate doesn't
        // validate — a hand-edited or older `schema.snapshot.json`
        // deserialized by `read_snapshot` — where a deterministic TEXT
        // column beats a panic. New built-ins need an arm added above.
        _ => "TEXT",
    }
}
