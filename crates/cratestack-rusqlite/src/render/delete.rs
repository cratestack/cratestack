//! Single-row DELETE rendering. Soft-delete-aware: becomes an
//! UPDATE-of-`deleted_at` when the descriptor declares one.

use std::fmt::Write;

use cratestack_sql::{Dialect, ModelDescriptor, SqlValue};

pub fn render_delete<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &ModelDescriptor<M, PK>,
    id: SqlValue,
    now: chrono::DateTime<chrono::Utc>,
) -> (String, Vec<SqlValue>) {
    if let Some(deleted_at) = descriptor.soft_delete_column {
        let mut sql = format!("UPDATE {} SET {deleted_at} = ", descriptor.table_name);
        dialect.write_placeholder(&mut sql, 1);
        let _ = write!(&mut sql, " WHERE {} = ", descriptor.primary_key);
        dialect.write_placeholder(&mut sql, 2);
        sql.push_str(" RETURNING ");
        sql.push_str(&descriptor.select_projection());
        return (sql, vec![SqlValue::DateTime(now), id]);
    }

    let mut sql = format!(
        "DELETE FROM {} WHERE {} = ",
        descriptor.table_name, descriptor.primary_key,
    );
    dialect.write_placeholder(&mut sql, 1);
    sql.push_str(" RETURNING ");
    sql.push_str(&descriptor.select_projection());
    (sql, vec![id])
}
