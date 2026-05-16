//! Single-row UPDATE rendering. SET columns emitted in caller order; PK
//! bound last.

use std::fmt::Write;

use cratestack_sql::{Dialect, ModelDescriptor, SqlColumnValue, SqlValue};

pub fn render_update<M, PK>(
    dialect: &dyn Dialect,
    descriptor: &ModelDescriptor<M, PK>,
    set: &[SqlColumnValue],
    id: SqlValue,
) -> (String, Vec<SqlValue>) {
    let mut sql = format!("UPDATE {} SET ", descriptor.table_name);
    let mut binds = Vec::with_capacity(set.len() + 1);
    let mut bind_index = 1usize;
    for (idx, value) in set.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        let _ = write!(&mut sql, "{} = ", value.column);
        dialect.write_placeholder(&mut sql, bind_index);
        bind_index += 1;
        binds.push(value.value.clone());
    }
    let _ = write!(&mut sql, " WHERE {} = ", descriptor.primary_key);
    dialect.write_placeholder(&mut sql, bind_index);
    binds.push(id);
    sql.push_str(" RETURNING ");
    sql.push_str(&descriptor.select_projection());
    (sql, binds)
}
