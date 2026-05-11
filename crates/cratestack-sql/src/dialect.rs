use std::fmt::Write;

/// Backend SQL dialect.
///
/// The cratestack renderers produce dialect-agnostic SQL for everything
/// except parameter placeholder syntax: Postgres uses `$1, $2, ...` while
/// SQLite uses `?1, ?2, ...` (also valid: `?` for positional). Implementors
/// own that one decision and the backend crates plug in their own impl.
///
/// Kept deliberately narrow — adding methods here forces every backend to
/// implement them, which is the wrong default. New dialect-specific quirks
/// should live in the backend's own renderer until at least two backends
/// agree on the shape.
pub trait Dialect {
    /// Write a numbered placeholder for bind index `index` (1-based) into
    /// `sql`. Postgres writes `$N`; SQLite writes `?N`.
    fn write_placeholder(&self, sql: &mut String, index: usize);
}

/// Postgres dialect — `$N` placeholders.
#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresDialect;

impl Dialect for PostgresDialect {
    fn write_placeholder(&self, sql: &mut String, index: usize) {
        let _ = write!(sql, "${index}");
    }
}

/// SQLite dialect — `?N` placeholders.
#[derive(Debug, Clone, Copy, Default)]
pub struct SqliteDialect;

impl Dialect for SqliteDialect {
    fn write_placeholder(&self, sql: &mut String, index: usize) {
        let _ = write!(sql, "?{index}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_dialect_writes_dollar_placeholders() {
        let dialect = PostgresDialect;
        let mut sql = String::new();
        dialect.write_placeholder(&mut sql, 1);
        sql.push_str(", ");
        dialect.write_placeholder(&mut sql, 2);
        assert_eq!(sql, "$1, $2");
    }

    #[test]
    fn sqlite_dialect_writes_question_placeholders() {
        let dialect = SqliteDialect;
        let mut sql = String::new();
        dialect.write_placeholder(&mut sql, 1);
        sql.push_str(", ");
        dialect.write_placeholder(&mut sql, 2);
        assert_eq!(sql, "?1, ?2");
    }
}
