//! Words a `@@map` / `@map` name may not be (cratestack#1375).
//!
//! No generated SQL quotes an identifier, so a mapped name that is a reserved
//! word in PostgreSQL or SQLite would read as the keyword (`FROM user` is
//! `current_user`) or fail to parse (`INSERT INTO order`). The list is the
//! union of PostgreSQL's reserved key words (the "reserved" and "reserved (can
//! be function or type)" rows of the SQL Key Words appendix, PostgreSQL 18),
//! SQLite's keyword list, and a few non-reserved words that still read oddly
//! unquoted. A column also may not be a PostgreSQL system column, which every
//! table already has. Quoting identifiers is a follow-up.

/// Sorted, lower case, space separated.
const RESERVED: &str = "
    abort action add after all alter always analyse analyze and any array as asc
    asymmetric attach authorization autoincrement before begin between binary both by
    cascade case cast check collate collation column commit concurrently conflict
    constraint create cross current current_catalog current_date current_role
    current_schema current_time current_timestamp current_user database default deferrable
    deferred delete desc detach distinct do drop each else end escape except exclude
    exclusive exists explain fail false fetch filter first following for foreign freeze
    from full generated glob grant group groups having if ignore ilike immediate in index
    indexed initially inner insert instead intersect into is isnull join key last lateral
    leading left like limit localtime localtimestamp match materialized natural no not
    nothing notnull null nulls of offset on only or order others outer over overlaps
    partition placing plan pragma preceding primary query raise range recursive references
    regexp reindex release rename replace restrict returning right rollback row rows
    savepoint select session_user set similar some symmetric system_user table tablesample
    temp temporary then ties to trailing transaction trigger true unbounded union unique
    update user using vacuum values variadic verbose view virtual when where window with
    within without
";

/// Columns every PostgreSQL table has (`ctid`, `xmin`, ...): a user column of
/// the same name is refused by the server, or hides the system one.
const SYSTEM_COLUMNS: &[&str] = &["cmax", "cmin", "ctid", "oid", "tableoid", "xmax", "xmin"];

/// Whether `name` (already lower case) is a reserved word in either dialect.
pub fn is_sql_reserved_word(name: &str) -> bool {
    RESERVED.split_whitespace().any(|word| word == name)
}

/// Whether `name` (already lower case) is a PostgreSQL system column.
pub fn is_sql_system_column(name: &str) -> bool {
    SYSTEM_COLUMNS.contains(&name)
}

#[cfg(test)]
#[path = "sql_reserved_words_tests.rs"]
mod tests;
