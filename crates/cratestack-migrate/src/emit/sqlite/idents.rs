//! Identifier quoting for SQLite DDL.

pub(super) fn quote_ident(name: &str) -> String {
    if is_reserved(name) {
        format!("\"{name}\"")
    } else {
        name.to_owned()
    }
}

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
