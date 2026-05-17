#![cfg(test)]

use crate::render::render_filter_expr_sql;

#[test]
fn coalesce_lte_renders_coalesce_function_with_one_bind() {
    let filter =
        cratestack_sql::coalesce(["next_attempt_at", "scheduled_at", "created_at"]).lte(42_i64);
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_filter_expr_sql(&filter, &mut sql, &mut bind_index);
    assert_eq!(
        sql,
        "COALESCE(next_attempt_at, scheduled_at, created_at) <= $1",
    );
    assert_eq!(bind_index, 2);
}

#[test]
fn coalesce_is_null_renders_no_bind() {
    let filter = cratestack_sql::coalesce(["a", "b"]).is_null();
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_filter_expr_sql(&filter, &mut sql, &mut bind_index);
    assert_eq!(sql, "COALESCE(a, b) IS NULL");
    assert_eq!(bind_index, 1, "IS NULL must not consume a bind slot");
}
