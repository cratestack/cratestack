//! Pure SQL-string preview helpers shared by the update primitives.
//! The output is a *sketch* — filter/policy clauses are placeholders
//! because the live SQL is built via sqlx's `QueryBuilder`. Used by
//! migration tooling and the schema studio when they need the rough
//! shape without an auth context.

/// Render the preview SQL for a bulk update-by-predicate.
pub fn render_update_many_preview_sql(
    table_name: &str,
    has_soft_delete: bool,
    version_column: Option<&str>,
    set_columns: &[&str],
    select_projection: &str,
) -> String {
    let mut sql = format!("UPDATE {table_name} SET ");
    for (idx, column) in set_columns.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(&format!("{column} = ${}", idx + 1));
    }
    if let Some(version_col) = version_column {
        sql.push_str(&format!(", {version_col} = {version_col} + 1"));
    }
    sql.push_str(" WHERE ");
    if has_soft_delete {
        sql.push_str("<soft_delete IS NULL> AND ");
    }
    sql.push_str("<filters> AND <update_policy> RETURNING ");
    sql.push_str(select_projection);
    sql
}

/// Render the SQL string for a per-row update.
pub fn render_update_preview_sql(
    table_name: &str,
    primary_key: &str,
    version_column: Option<&str>,
    columns: &[&str],
    select_projection: &str,
) -> String {
    let assignments = columns
        .iter()
        .enumerate()
        .map(|(index, column)| format!("{column} = ${}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");

    match version_column {
        Some(version_col) => format!(
            "UPDATE {} SET {}, {} = {} + 1 WHERE {} = ${} AND {} = ${} RETURNING {}",
            table_name,
            assignments,
            version_col,
            version_col,
            primary_key,
            columns.len() + 1,
            version_col,
            columns.len() + 2,
            select_projection,
        ),
        None => format!(
            "UPDATE {} SET {} WHERE {} = ${} RETURNING {}",
            table_name,
            assignments,
            primary_key,
            columns.len() + 1,
            select_projection,
        ),
    }
}
