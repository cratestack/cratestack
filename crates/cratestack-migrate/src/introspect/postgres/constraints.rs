//! `pg_constraint`: primary keys (`contype = 'p'`) and CHECK
//! constraints (`contype = 'c'`).
//!
//! A CHECK constraint's predicate, as Postgres deparses it via
//! `pg_get_constraintdef`, is captured as [`CheckKind::Raw`] — opaque
//! SQL text, never reverse-mapped to a validator attribute (design doc
//! §2.2, §5.2) — *unless* it matches the exact shape
//! `crate::emit::postgres::checks::render_check_predicate_postgres`
//! produces for `CheckKind::Enum` and Postgres round-trips it back as
//! (`col = ANY (ARRAY[...])` for scalar membership, `col <@ ARRAY[...]`
//! for list containment — verified against a live Postgres 18
//! instance; Postgres normalises a hand-written `col IN (...)` into
//! the `= ANY` form when deparsing, so both a cratestack-authored
//! migration and a hand-written equivalent land on the same text).
//! Recognising that one shape lets a table whose enum CHECK cratestack
//! itself would have emitted round-trip exactly, rather than showing
//! spurious `CheckKind::Raw` vs. `CheckKind::Enum` drift on every
//! enum-backed table. The pattern-matching itself lives in
//! [`super::check_pattern`].

use sqlx_core::row::Row as _;
use sqlx_postgres::PgPool;

use crate::ir::{AddCheck, CheckKind};

use super::check_pattern::reconstruct_enum;
use super::error::IntrospectError;

pub(super) async fn introspect_primary_key(
    pool: &PgPool,
    table: &str,
) -> Result<Vec<String>, IntrospectError> {
    let rows = sqlx_core::query::query(
        "SELECT a.attname \
         FROM pg_constraint c \
         JOIN LATERAL unnest(c.conkey) WITH ORDINALITY AS ck(attnum, ord) ON true \
         JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = ck.attnum \
         WHERE c.conrelid = to_regclass($1) AND c.contype = 'p' \
         ORDER BY ck.ord",
    )
    .bind(table)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| row.try_get::<String, _>(0).map_err(IntrospectError::from))
        .collect()
}

pub(super) async fn introspect_checks(
    pool: &PgPool,
    table: &str,
) -> Result<Vec<AddCheck>, IntrospectError> {
    let rows = sqlx_core::query::query(
        "SELECT c.conname, pg_get_constraintdef(c.oid) AS predicate, \
                (SELECT a.attname \
                   FROM pg_attribute a \
                  WHERE a.attrelid = c.conrelid AND a.attnum = c.conkey[1]) AS single_column \
         FROM pg_constraint c \
         WHERE c.conrelid = to_regclass($1) \
           AND c.contype = 'c' \
           AND array_length(c.conkey, 1) = 1 \
         ORDER BY c.conname",
    )
    .bind(table)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let name: String = row.try_get(0)?;
        let raw_def: String = row.try_get(1)?;
        let column: String = row.try_get(2)?;
        let predicate = strip_check_wrapper(&raw_def);
        let kind = reconstruct_enum(&column, predicate)
            .unwrap_or_else(|| CheckKind::Raw(predicate.to_owned()));
        out.push(AddCheck {
            name,
            table: table.to_owned(),
            column,
            kind,
        });
    }
    Ok(out)
}

/// `pg_get_constraintdef` always renders `CHECK (<predicate>)` for a
/// CHECK constraint (see the Postgres source for `ConstraintDef`) —
/// strip that fixed wrapper. `<predicate>` may still carry its own
/// nested/redundant parens; `check_pattern::reconstruct_enum` normalises
/// those separately.
fn strip_check_wrapper(def: &str) -> &str {
    def.strip_prefix("CHECK (")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(def)
}
