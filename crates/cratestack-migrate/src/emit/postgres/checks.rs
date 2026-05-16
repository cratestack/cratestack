//! CHECK-constraint DDL: ADD / DROP plus predicate rendering for
//! `@range`, `@length`, `@iso4217` carried into the runtime via
//! `@db_enforce`.

use std::fmt::Write as _;

use crate::ir::{AddCheck, CheckKind, DropCheck};

use super::idents::quote_ident;

pub(super) fn emit_add_check(sql: &mut String, check: &AddCheck) {
    writeln!(
        sql,
        "ALTER TABLE {} ADD CONSTRAINT {} CHECK ({});",
        quote_ident(&check.table),
        quote_ident(&check.name),
        render_check_predicate_postgres(&check.column, &check.kind)
    )
    .unwrap();
}

pub(super) fn emit_drop_check(sql: &mut String, check: &DropCheck) {
    writeln!(
        sql,
        "ALTER TABLE {} DROP CONSTRAINT {};",
        quote_ident(&check.table),
        quote_ident(&check.name)
    )
    .unwrap();
}

fn render_check_predicate_postgres(column: &str, kind: &CheckKind) -> String {
    let c = quote_ident(column);
    match kind {
        CheckKind::Range { min, max } => match (min, max) {
            (Some(min), Some(max)) => format!("{c} >= {min} AND {c} <= {max}"),
            (Some(min), None) => format!("{c} >= {min}"),
            (None, Some(max)) => format!("{c} <= {max}"),
            (None, None) => "TRUE".to_owned(),
        },
        CheckKind::Length { min, max } => match (min, max) {
            (Some(min), Some(max)) => format!("length({c}) BETWEEN {min} AND {max}"),
            (Some(min), None) => format!("length({c}) >= {min}"),
            (None, Some(max)) => format!("length({c}) <= {max}"),
            (None, None) => "TRUE".to_owned(),
        },
        CheckKind::Iso4217 => format!("{c} ~ '^[A-Z]{{3}}$'"),
    }
}
