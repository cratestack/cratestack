#![cfg(test)]

use crate::query::render_update_preview_sql;

#[test]
fn update_preview_sql_unversioned_renders_simple_where_clause() {
    let sql = render_update_preview_sql(
        "accounts",
        "id",
        None,
        &["balance", "updated_at"],
        "id AS \"id\", balance AS \"balance\"",
    );
    assert_eq!(
        sql,
        "UPDATE accounts SET balance = $1, updated_at = $2 WHERE id = $3 RETURNING id AS \"id\", balance AS \"balance\""
    );
}

#[test]
fn update_preview_sql_versioned_bumps_version_and_filters_on_expected() {
    let sql = render_update_preview_sql(
        "accounts",
        "id",
        Some("version"),
        &["balance"],
        "id AS \"id\", version AS \"version\"",
    );
    assert_eq!(
        sql,
        "UPDATE accounts SET balance = $1, version = version + 1 WHERE id = $2 AND version = $3 RETURNING id AS \"id\", version AS \"version\""
    );
}
