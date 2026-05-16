#![cfg(test)]

use crate::query::render_update_many_preview_sql;

#[test]
fn update_many_preview_sql_unversioned_no_soft_delete() {
    let sql = render_update_many_preview_sql(
        "posts",
        false,
        None,
        &["title", "published"],
        "id AS \"id\", title AS \"title\"",
    );
    assert_eq!(
        sql,
        "UPDATE posts SET title = $1, published = $2 WHERE <filters> AND <update_policy> RETURNING id AS \"id\", title AS \"title\"",
    );
}

#[test]
fn update_many_preview_sql_versioned_bumps_version() {
    let sql = render_update_many_preview_sql(
        "accounts",
        false,
        Some("version"),
        &["balance"],
        "id AS \"id\", balance AS \"balance\"",
    );
    assert_eq!(
        sql,
        "UPDATE accounts SET balance = $1, version = version + 1 WHERE <filters> AND <update_policy> RETURNING id AS \"id\", balance AS \"balance\"",
    );
}

#[test]
fn update_many_preview_sql_with_soft_delete_layers_in_predicate() {
    let sql = render_update_many_preview_sql(
        "posts",
        true,
        None,
        &["title"],
        "id AS \"id\"",
    );
    assert_eq!(
        sql,
        "UPDATE posts SET title = $1 WHERE <soft_delete IS NULL> AND <filters> AND <update_policy> RETURNING id AS \"id\"",
    );
}
