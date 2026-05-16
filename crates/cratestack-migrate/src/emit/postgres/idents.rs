//! Identifier quoting for Postgres DDL.
//!
//! Quoting is intentionally minimal: model-derived identifiers
//! (`customers`, `order_count`) are pure snake_case and Postgres-safe
//! without quotes. Only names that collide with reserved words get
//! double-quoted.

pub(super) fn quote_ident(name: &str) -> String {
    if is_reserved(name) {
        format!("\"{name}\"")
    } else {
        name.to_owned()
    }
}

/// Postgres reserved words that show up in `.cstack` table/column
/// names often enough to be worth quoting. Not the full SQL reserved
/// list — that would require quoting nearly everything. The macro
/// codegen already quotes these in queries, so the migration table
/// matches.
fn is_reserved(name: &str) -> bool {
    matches!(
        name,
        "order"
            | "user"
            | "group"
            | "select"
            | "from"
            | "where"
            | "table"
            | "column"
            | "default"
            | "type"
            | "primary"
            | "foreign"
            | "references"
            | "constraint"
            | "check"
            | "unique"
            | "index"
            | "view"
            | "schema"
    )
}
