//! Index DDL: CREATE INDEX and DROP INDEX.

use std::fmt::Write as _;

use crate::ir::{AddIndex, DropIndex};

use super::idents::quote_ident;

/// Renders `CREATE [UNIQUE] INDEX name ON table [USING method] (columns)
/// [WHERE predicate];` — the `USING` clause, each column's operator
/// class (issue #156), and the trailing `WHERE` clause (issue #742,
/// partial indexes) only appear when `index.using`/`index.opclass`/
/// `index.where_predicate` are `Some`, so a plain `AddIndex` (every
/// pre-existing `@unique`/`@@unique([...])`-derived index) renders
/// byte-identical DDL to before these fields existed. `where_predicate`
/// is rendered verbatim — not re-quoted or otherwise transformed — same
/// posture as `using`/`opclass`: see `docs/design/extensions.md` §2/§6.
pub(super) fn emit_add_index(sql: &mut String, index: &AddIndex) {
    let unique = if index.unique { "UNIQUE " } else { "" };
    let using_clause = match index.using.as_deref() {
        Some(method) => format!(" USING {}", quote_ident(method)),
        None => String::new(),
    };
    let columns: Vec<String> = index
        .columns
        .iter()
        .map(|column| render_index_column(column, index.opclass.as_deref()))
        .collect();
    let where_clause = match index.where_predicate.as_deref() {
        Some(predicate) => format!(" WHERE {predicate}"),
        None => String::new(),
    };
    writeln!(
        sql,
        "CREATE {unique}INDEX {} ON {}{using_clause} ({}){where_clause};",
        quote_ident(&index.name),
        quote_ident(&index.table),
        columns.join(", ")
    )
    .unwrap();
}

/// One column entry inside an index's column list: `col` alone, or
/// `col opclass` when an operator class was declared (`@@index([...],
/// opclass: "...")`). Postgres applies the same operator class syntax to
/// every column position — see `CREATE INDEX`'s grammar — so a single
/// `opclass` on the `AddIndex` op is applied to each listed column.
fn render_index_column(column: &str, opclass: Option<&str>) -> String {
    match opclass {
        Some(opclass) => format!("{} {}", quote_ident(column), quote_ident(opclass)),
        None => quote_ident(column),
    }
}

pub(super) fn emit_drop_index(sql: &mut String, drop: &DropIndex) {
    writeln!(sql, "DROP INDEX {};", quote_ident(&drop.name)).unwrap();
}
