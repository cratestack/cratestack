//! Which columns an `UPDATE` cannot change without taking `FOR UPDATE`.

use super::ModelDescriptor;

impl<M, PK> ModelDescriptor<M, PK> {
    /// Record the model's `@unique` / `@@unique` columns. A separate step from
    /// [`ModelDescriptor::new`], like [`ModelDescriptor::with_subscribed`].
    pub const fn with_unique_columns(self, unique_columns: &'static [&'static str]) -> Self {
        Self {
            unique_columns,
            ..self
        }
    }

    /// Whether changing `column` is a key update: the primary key or a unique
    /// column. Postgres then takes `FOR UPDATE` on the row, not `FOR NO KEY
    /// UPDATE`, because a foreign key may point at the old value.
    pub fn is_key_column(&self, column: &str) -> bool {
        column == self.primary_key || self.unique_columns.contains(&column)
    }
}
