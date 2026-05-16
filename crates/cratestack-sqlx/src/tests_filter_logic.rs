#![cfg(test)]

use crate::{FieldRef, FilterExpr, render::render_filter_expr_sql};

#[test]
fn filter_expr_and_or_flattens_matching_groups() {
    let left = FilterExpr::from(FieldRef::<(), i64>::new("id").eq(1_i64));
    let right = FilterExpr::from(FieldRef::<(), bool>::new("published").is_true());
    let third = FilterExpr::from(FieldRef::<(), String>::new("title").contains("Post"));

    let and_expr = left.clone().and(right.clone()).and(third.clone());
    let or_expr = left.or(right).or(third);

    assert!(matches!(and_expr, FilterExpr::All(filters) if filters.len() == 3));
    assert!(matches!(or_expr, FilterExpr::Any(filters) if filters.len() == 3));
}

#[test]
fn filter_expr_not_wraps_and_unwraps_double_negation() {
    let filter = FilterExpr::from(FieldRef::<(), bool>::new("published").is_true());
    let negated = filter.clone().not();
    let restored = negated.clone().not();
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_filter_expr_sql(&negated, &mut sql, &mut bind_index);

    assert_eq!(sql, "NOT (published = $1)");
    assert_eq!(restored, filter);
}
