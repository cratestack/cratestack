//! Column-name translation: a descriptor holds SQL names (`@map`) where a
//! serialized model is keyed by field names (cratestack#1375).

use super::ModelDescriptor;

impl<M, PK> ModelDescriptor<M, PK> {
    /// The field name of the stored column called `sql`, or `None` when this
    /// model has no such column. A model's JSON snapshot is keyed by the field
    /// name; only the SQL name is mapped (`@map`), so a caller holding a
    /// `pii_columns` / `primary_key` entry translates it here first.
    pub fn rust_name_of(&self, sql: &str) -> Option<&'static str> {
        self.columns
            .iter()
            .find(|column| column.sql_name == sql)
            .map(|column| column.rust_name)
    }

    /// [`Self::rust_name_of`], or `sql` itself when the column is unknown.
    pub fn rust_name_or<'a>(&self, sql: &'a str) -> &'a str {
        self.rust_name_of(sql).unwrap_or(sql)
    }
}
