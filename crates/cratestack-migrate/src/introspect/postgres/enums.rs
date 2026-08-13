//! Native Postgres enum types (`pg_enum`/`pg_type`), folded into a
//! column CHECK — matching `crate::convert::enum_check_kind`'s shape
//! (per the note carried over from issue #203's own report: enums
//! aren't a separate bucket on [`crate::Projections`], they're baked
//! into the owning column's `TableProjection::checks`).
//!
//! `cratestack`'s own Postgres emitter never creates a native
//! `CREATE TYPE ... AS ENUM` — every `.cstack` enum field is stored as
//! `TEXT` plus a membership `CHECK` (issue #228, see
//! `crate::emit::postgres::checks`) — but nothing stops a hand-created
//! table this crate is asked to introspect from using one. When it
//! does, this module recovers the same [`CheckKind::Enum`] the
//! schema-side projection would have produced for an equivalent
//! `.cstack` `enum` field, so `diff_projections` doesn't see false
//! drift on the CHECK. The column itself still projects as
//! `Scalar("String")` (see `super::columns::resolve_column`) — the
//! `.cstack`-side enum *name* has no catalog representation to recover
//! it from, which is the same documented lossiness the design doc's
//! §2.2 already calls out for validator-derived checks.

use sqlx_core::row::Row as _;
use sqlx_postgres::PgPool;

use crate::ir::{AddCheck, CheckKind};
use crate::naming::check_name;

use super::error::IntrospectError;

/// One [`AddCheck`] per column of `table` whose type is a native
/// Postgres enum (`pg_type.typtype = 'e'`), in column order.
///
/// Array-of-native-enum columns are out of scope: `pg_enum.enumtypid`
/// names the *element* type, not the derived array type Postgres
/// stores such a column under, so this query's join against `atttypid`
/// naturally excludes them — they surface as unmapped columns instead
/// (`typcategory = 'A'`, see `super::types::map_scalar`), which is the
/// correct conservative outcome rather than a silent gap.
pub(super) async fn introspect_enum_checks(
    pool: &PgPool,
    table: &str,
) -> Result<Vec<AddCheck>, IntrospectError> {
    let rows = sqlx_core::query::query(
        "SELECT a.attname, e.enumlabel \
         FROM pg_attribute a \
         JOIN pg_type t ON t.oid = a.atttypid \
         JOIN pg_enum e ON e.enumtypid = t.oid \
         WHERE a.attrelid = to_regclass($1) \
           AND a.attnum > 0 \
           AND NOT a.attisdropped \
         ORDER BY a.attname, e.enumsortorder",
    )
    .bind(table)
    .fetch_all(pool)
    .await?;

    let mut variants_by_column: Vec<(String, Vec<String>)> = Vec::new();
    for row in rows {
        let column: String = row.try_get(0)?;
        let variant: String = row.try_get(1)?;
        match variants_by_column.last_mut() {
            Some((name, variants)) if *name == column => variants.push(variant),
            _ => variants_by_column.push((column, vec![variant])),
        }
    }

    Ok(variants_by_column
        .into_iter()
        .filter(|(_, variants)| !variants.is_empty())
        .map(|(column, variants)| AddCheck {
            name: check_name(table, &column, "enum"),
            table: table.to_owned(),
            column: column.clone(),
            kind: CheckKind::Enum {
                variants,
                list: false,
            },
        })
        .collect())
}
