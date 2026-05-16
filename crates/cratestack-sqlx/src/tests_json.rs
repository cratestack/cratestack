#![cfg(test)]

use crate::{FieldRef, render::render_filter_expr_sql};

#[test]
fn json_has_key_renders_question_operator_with_one_bind() {
    let filter = FieldRef::<(), serde_json::Value>::new("metrics").json_has_key("loss");
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_filter_expr_sql(&filter, &mut sql, &mut bind_index);
    assert_eq!(sql, "metrics ? $1");
    assert_eq!(bind_index, 2);
}

#[test]
fn json_get_text_eq_renders_arrow_arrow_operator() {
    let filter = FieldRef::<(), serde_json::Value>::new("metrics")
        .json_get_text("loss")
        .eq("0.001");
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_filter_expr_sql(&filter, &mut sql, &mut bind_index);
    assert_eq!(sql, "metrics ->> $1 = $2");
    assert_eq!(bind_index, 3);
}

#[test]
fn json_get_text_is_not_null_renders_no_value_bind() {
    let filter = FieldRef::<(), serde_json::Value>::new("metrics")
        .json_get_text("loss")
        .is_not_null();
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_filter_expr_sql(&filter, &mut sql, &mut bind_index);
    assert_eq!(sql, "metrics ->> $1 IS NOT NULL");
    assert_eq!(bind_index, 2, "only the key consumes a bind slot");
}
