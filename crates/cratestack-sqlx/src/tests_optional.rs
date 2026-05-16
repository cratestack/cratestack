#![cfg(test)]

use crate::{FieldRef, FilterExpr, render::render_filter_expr_sql};

#[test]
fn eq_or_null_preview_emits_two_branch_disjunction_with_one_bind() {
    let filter = FieldRef::<(), String>::new("market_code").eq_or_null("us");
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_filter_expr_sql(&FilterExpr::from(filter), &mut sql, &mut bind_index);
    assert_eq!(sql, "(market_code IS NULL OR market_code = $1)");
    assert_eq!(bind_index, 2, "exactly one bind consumed");
}

#[test]
fn match_optional_some_emits_eq_or_null_clause() {
    let filter = FieldRef::<(), String>::new("market_code")
        .match_optional(Some("eu"))
        .expect("Some should produce a filter");
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_filter_expr_sql(&FilterExpr::from(filter), &mut sql, &mut bind_index);
    assert!(sql.contains("market_code IS NULL OR market_code = $1"));
}

#[test]
fn match_optional_none_yields_no_filter() {
    let filter: Option<crate::Filter> =
        FieldRef::<(), String>::new("market_code").match_optional(Option::<&str>::None);
    assert!(filter.is_none(), "None input must yield None filter");
}
