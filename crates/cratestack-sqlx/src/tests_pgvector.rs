#![cfg(test)]

//! SQL-generation coverage for the pgvector distance-operator query
//! builder (cratestack#163) — one test per metric for both the
//! `ORDER BY` shape and the threshold-filter shape, mirroring
//! `tests_geography.rs`'s "preview string" approach so these run with
//! no live database and no `pgvector` Cargo feature. The real bind
//! path (via `sqlx::QueryBuilder`, gated on the `pgvector` feature) is
//! covered separately below.

use cratestack_sql::VectorMetric;

use crate::{
    FieldRef, OrderClause, SortDirection,
    render::{render_filter_expr_sql, render_order_clause_sql},
};

fn order_sql(clause: &OrderClause) -> (String, usize) {
    let mut sql = String::new();
    let mut bind_index = 1usize;
    render_order_clause_sql(clause, &mut sql, &mut bind_index);
    (sql, bind_index)
}

#[test]
fn order_by_distance_l2_renders_arrow_operator() {
    let clause = FieldRef::<(), Vec<f32>>::new("embedding")
        .order_by_distance(VectorMetric::L2, vec![0.1, 0.2, 0.3]);
    let (sql, bind_index) = order_sql(&clause);
    assert_eq!(sql, "(embedding <-> $1) ASC NULLS LAST");
    assert_eq!(bind_index, 2);
}

#[test]
fn order_by_distance_cosine_renders_cosine_operator() {
    let clause = FieldRef::<(), Vec<f32>>::new("embedding")
        .order_by_distance(VectorMetric::Cosine, vec![0.1, 0.2, 0.3]);
    let (sql, _) = order_sql(&clause);
    assert_eq!(sql, "(embedding <=> $1) ASC NULLS LAST");
}

#[test]
fn order_by_distance_inner_product_renders_inner_product_operator() {
    let clause = FieldRef::<(), Vec<f32>>::new("embedding")
        .order_by_distance(VectorMetric::InnerProduct, vec![0.1, 0.2, 0.3]);
    let (sql, _) = order_sql(&clause);
    assert_eq!(sql, "(embedding <#> $1) ASC NULLS LAST");
}

#[test]
fn distance_to_desc_orders_farthest_first() {
    let clause = FieldRef::<(), Vec<f32>>::new("embedding")
        .distance_to(VectorMetric::L2, vec![0.1])
        .desc();
    let (sql, _) = order_sql(&clause);
    assert_eq!(sql, "(embedding <-> $1) DESC NULLS LAST");
}

#[test]
fn distance_filter_lte_renders_threshold_comparison_with_two_binds() {
    let filter = FieldRef::<(), Vec<f32>>::new("embedding")
        .distance_to(VectorMetric::L2, vec![0.1, 0.2])
        .lte(0.75_f64);
    let mut sql = String::new();
    let mut bind_index = 1usize;
    render_filter_expr_sql(&filter, &mut sql, &mut bind_index);
    assert_eq!(sql, "(embedding <-> $1) <= $2");
    assert_eq!(bind_index, 3);
}

#[test]
fn distance_filter_supports_all_three_metrics() {
    for (metric, operator) in [
        (VectorMetric::L2, "<->"),
        (VectorMetric::Cosine, "<=>"),
        (VectorMetric::InnerProduct, "<#>"),
    ] {
        let filter = FieldRef::<(), Vec<f32>>::new("embedding")
            .distance_to(metric, vec![1.0])
            .lt(1.0_f64);
        let mut sql = String::new();
        let mut bind_index = 1usize;
        render_filter_expr_sql(&filter, &mut sql, &mut bind_index);
        assert_eq!(sql, format!("(embedding {operator} $1) < $2"));
    }
}

#[test]
fn optional_vector_field_exposes_the_same_distance_builder() {
    let clause = FieldRef::<(), Option<Vec<f32>>>::new("embedding")
        .order_by_distance(VectorMetric::Cosine, vec![0.5]);
    assert_eq!(clause.direction, SortDirection::Asc);
}

/// The real `sqlx::QueryBuilder` bind path — verifies the query vector
/// actually binds as a real `pgvector::Vector` (not just a string
/// placeholder) without panicking, and that the generated SQL text
/// matches the preview renderer above. `QueryBuilder::sql()` reads back
/// the SQL assembled so far without needing a live connection.
#[cfg(feature = "pgvector")]
mod real_bind_path {
    use cratestack_sql::VectorMetric;

    use crate::query::{push_filter_query, push_order_and_paging};
    use crate::{FieldRef, sqlx};

    #[test]
    fn order_by_distance_binds_a_real_pgvector_value() {
        let clause = FieldRef::<(), Vec<f32>>::new("embedding")
            .order_by_distance(VectorMetric::L2, vec![0.1, 0.2, 0.3]);
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT * FROM documents");
        push_order_and_paging(&mut query, std::slice::from_ref(&clause), None, None);
        assert_eq!(
            query.sql(),
            "SELECT * FROM documents ORDER BY (embedding <-> $1) ASC NULLS LAST",
        );
    }

    #[test]
    fn distance_filter_binds_query_vector_and_threshold() {
        let filter = FieldRef::<(), Vec<f32>>::new("embedding")
            .distance_to(VectorMetric::Cosine, vec![0.1, 0.2])
            .lte(0.5_f64);
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT * FROM documents WHERE ");
        push_filter_query(&mut query, std::slice::from_ref(&filter));
        assert_eq!(
            query.sql(),
            "SELECT * FROM documents WHERE (embedding <=> $1) <= $2",
        );
    }
}
