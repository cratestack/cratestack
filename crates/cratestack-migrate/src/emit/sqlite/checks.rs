//! CHECK-constraint comments.
//!
//! SQLite has no `ALTER TABLE ADD CONSTRAINT`; both ADD and DROP
//! require a full table rebuild. The emitter writes a marker comment
//! so the developer notices and hand-writes the rebuild in
//! `up.pre.sql`.

use std::fmt::Write as _;

use crate::ir::{AddCheck, CheckKind, DropCheck};

use super::idents::quote_ident;

pub(super) fn emit_add_check(sql: &mut String, check: &AddCheck) {
    if is_enum_membership(&check.kind) {
        return;
    }
    writeln!(
        sql,
        "-- SQLite: ADD CONSTRAINT {} CHECK ({}) — \
         requires table rebuild on SQLite. Hand-write up.pre.sql.",
        check.name,
        render_check_predicate_sqlite(&check.column, &check.kind)
    )
    .unwrap();
}

pub(super) fn emit_drop_check(sql: &mut String, check: &DropCheck) {
    if is_enum_membership(&check.kind) {
        return;
    }
    writeln!(
        sql,
        "-- SQLite: DROP CONSTRAINT {} — requires table rebuild on SQLite.",
        check.name
    )
    .unwrap();
}

/// Enum membership constraints have no SQLite footprint, so they emit
/// nothing at all — not even the table-rebuild marker the validator
/// checks leave behind.
///
/// Every SQLite column is declared `BLOB` (see the module docs), which
/// applies no type or domain constraint of its own, and the generated
/// rusqlite row decoder already rejects an unknown variant when it
/// `.parse()`s the stored string. Emitting a "hand-write a table
/// rebuild" marker for a constraint the framework derives
/// automatically — and which SQLite never had — would be noise on
/// every enum column. This preserves the pre-existing contract that
/// enums have no SQLite footprint (issue #228 changed only Postgres).
fn is_enum_membership(kind: &CheckKind) -> bool {
    matches!(kind, CheckKind::Enum { .. })
}

fn render_check_predicate_sqlite(column: &str, kind: &CheckKind) -> String {
    let c = quote_ident(column);
    match kind {
        CheckKind::Range { min, max } => match (min, max) {
            (Some(min), Some(max)) => format!("{c} >= {min} AND {c} <= {max}"),
            (Some(min), None) => format!("{c} >= {min}"),
            (None, Some(max)) => format!("{c} <= {max}"),
            (None, None) => "1".to_owned(),
        },
        CheckKind::Length { min, max } => match (min, max) {
            (Some(min), Some(max)) => format!("length({c}) BETWEEN {min} AND {max}"),
            (Some(min), None) => format!("length({c}) >= {min}"),
            (None, Some(max)) => format!("length({c}) <= {max}"),
            (None, None) => "1".to_owned(),
        },
        // SQLite GLOB is closer to LIKE than regex; this is good
        // enough for ISO 4217 codes which are exactly 3 uppercase
        // ASCII letters.
        CheckKind::Iso4217 => format!("{c} GLOB '[A-Z][A-Z][A-Z]'"),
        // Not reached — `is_enum_membership` short-circuits both
        // callers before they render a predicate. Rendered correctly
        // anyway so the match carries no unreachable panic. SQLite has
        // no array type, so the `list` case has no distinct form here.
        CheckKind::Enum { variants, .. } => {
            let literals: Vec<String> = variants
                .iter()
                .map(|variant| format!("'{}'", variant.replace('\'', "''")))
                .collect();
            format!("{c} IN ({})", literals.join(", "))
        }
    }
}
