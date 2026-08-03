//! Column introspection: `pg_attribute` + `pg_type` (+ `pg_attrdef` for
//! defaults), mapped to [`Column`] via [`super::types::map_scalar`].
//!
//! Native Postgres enum columns (`pg_type.typtype = 'e'`) are handled
//! by [`super::enums`], not here — this module treats them as an
//! ordinary mapped `String` column (see that module's doc comment for
//! why) and leaves the membership CHECK to be attached separately.

use sqlx_core::row::Row as _;
use sqlx_postgres::PgPool;

use crate::ir::{Column, ColumnArity, ColumnDefault, ColumnType};

use super::error::IntrospectError;
use super::types::map_scalar;

/// One column's resolution: either a [`Column`] this crate's IR can
/// represent, or an unmapped-type report (design doc §5.2 — "unmapped,
/// never guessed").
pub(super) enum ColumnOutcome {
    Mapped(Column),
    Unmapped {
        column: String,
        postgres_type: String,
    },
}

struct Row {
    name: String,
    typname: String,
    typtype: String,
    typcategory: String,
    not_null: bool,
    default_expr: Option<String>,
}

pub(super) async fn introspect_columns(
    pool: &PgPool,
    table: &str,
) -> Result<Vec<ColumnOutcome>, IntrospectError> {
    let rows = sqlx_core::query::query(
        "SELECT a.attname, t.typname, t.typtype::text, t.typcategory::text, \
                a.attnotnull, pg_get_expr(ad.adbin, ad.adrelid) AS default_expr \
         FROM pg_attribute a \
         JOIN pg_type t ON t.oid = a.atttypid \
         LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
         WHERE a.attrelid = to_regclass($1) \
           AND a.attnum > 0 \
           AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
    .bind(table)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let parsed = Row {
            name: row.try_get(0)?,
            typname: row.try_get(1)?,
            typtype: row.try_get(2)?,
            typcategory: row.try_get(3)?,
            not_null: row.try_get(4)?,
            default_expr: row.try_get(5)?,
        };
        out.push(resolve_column(parsed));
    }
    Ok(out)
}

fn resolve_column(row: Row) -> ColumnOutcome {
    let typtype = row.typtype.chars().next().unwrap_or('\0');
    let typcategory = row.typcategory.chars().next().unwrap_or('\0');

    // Native enum columns are mapped elsewhere (`super::enums`); here
    // they're treated exactly like a `text` column — see that module's
    // doc comment for why `Scalar("String")` is the honest projection.
    let scalar = if typtype == 'e' {
        Some("String")
    } else {
        map_scalar(&row.typname, typtype, typcategory)
    };

    let Some(scalar) = scalar else {
        return ColumnOutcome::Unmapped {
            column: row.name,
            postgres_type: row.typname,
        };
    };

    ColumnOutcome::Mapped(Column {
        name: row.name,
        ty: ColumnType::Scalar(scalar.to_owned()),
        arity: if row.not_null {
            ColumnArity::Required
        } else {
            ColumnArity::Optional
        },
        default: row.default_expr.as_deref().map(parse_default),
        // Set by the caller once the primary key columns are known
        // (`pg_constraint` and `pg_attribute` are queried separately).
        primary_key: false,
    })
}

/// Best-effort classification of a `pg_get_expr`-rendered default
/// expression into [`ColumnDefault`], mirroring
/// `crate::convert::fields`'s literal-vs-function split on the
/// `.cstack` side. Not a general SQL parser: Postgres always appends
/// an explicit `::type` cast to a literal default
/// (`'active'::text`, `0::bigint`), which `.cstack`-side defaults never
/// carry, so a single trailing cast is stripped first. Anything more
/// exotic (a default with its own literal `::` inside a string, a
/// multi-cast chain) is passed through unchanged — this is a known,
/// documented source of possible spurious `AlterColumnDefault` drift
/// in a Phase C report, not a correctness bug in the mapped/unmapped
/// column classification above.
fn parse_default(expr: &str) -> ColumnDefault {
    let stripped = strip_trailing_cast(expr);
    if stripped.ends_with(')') && !is_quoted(&stripped) {
        ColumnDefault::Function(stripped)
    } else {
        ColumnDefault::Literal(stripped)
    }
}

fn strip_trailing_cast(expr: &str) -> String {
    let Some(idx) = expr.rfind("::") else {
        return expr.to_owned();
    };
    let (head, cast) = expr.split_at(idx);
    let type_name = &cast[2..];
    let looks_like_type_name = !type_name.is_empty()
        && type_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '[' | ']' | '.' | ' '));
    if looks_like_type_name {
        head.to_owned()
    } else {
        expr.to_owned()
    }
}

fn is_quoted(value: &str) -> bool {
    value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_a_single_trailing_type_cast() {
        assert_eq!(strip_trailing_cast("'active'::text"), "'active'");
        assert_eq!(strip_trailing_cast("0::bigint"), "0");
    }

    #[test]
    fn classifies_function_calls_vs_literals() {
        assert_eq!(
            parse_default("now()"),
            ColumnDefault::Function("now()".into())
        );
        assert_eq!(
            parse_default("'pending'::text"),
            ColumnDefault::Literal("'pending'".into())
        );
        assert_eq!(
            parse_default("0::bigint"),
            ColumnDefault::Literal("0".into())
        );
        assert_eq!(parse_default("true"), ColumnDefault::Literal("true".into()));
    }
}
