use super::*;

#[test]
fn sql_operator_matches_pgvector_operators() {
    assert_eq!(VectorMetric::L2.sql_operator(), "<->");
    assert_eq!(VectorMetric::Cosine.sql_operator(), "<=>");
    assert_eq!(VectorMetric::InnerProduct.sql_operator(), "<#>");
}

#[test]
fn from_opclass_maps_known_pgvector_opclasses() {
    assert_eq!(
        VectorMetric::from_opclass("vector_l2_ops"),
        Some(VectorMetric::L2)
    );
    assert_eq!(
        VectorMetric::from_opclass("vector_cosine_ops"),
        Some(VectorMetric::Cosine)
    );
    assert_eq!(
        VectorMetric::from_opclass("vector_ip_ops"),
        Some(VectorMetric::InnerProduct)
    );
    assert_eq!(VectorMetric::from_opclass("btree"), None);
    assert_eq!(VectorMetric::from_opclass(""), None);
}

#[test]
fn asc_builds_a_vector_distance_order_clause() {
    let clause = VectorDistanceExpr::new("embedding", VectorMetric::L2, vec![0.1, 0.2]).asc();
    assert_eq!(clause.direction, SortDirection::Asc);
    assert_eq!(clause.null_order, NullOrder::Last);
    match clause.target {
        OrderTarget::VectorDistance {
            column,
            metric,
            query_vector,
        } => {
            assert_eq!(column, "embedding");
            assert_eq!(metric, VectorMetric::L2);
            assert_eq!(query_vector, vec![0.1, 0.2]);
        }
        other => panic!("expected OrderTarget::VectorDistance, got {other:?}"),
    }
}

#[test]
fn desc_flips_the_sort_direction() {
    let clause = VectorDistanceExpr::new("embedding", VectorMetric::Cosine, vec![1.0]).desc();
    assert_eq!(clause.direction, SortDirection::Desc);
}

#[test]
fn lt_builds_a_vector_distance_filter_expr() {
    let expr =
        VectorDistanceExpr::new("embedding", VectorMetric::InnerProduct, vec![1.0]).lt(0.5_f64);
    match expr {
        FilterExpr::VectorDistance(filter) => {
            assert_eq!(filter.column, "embedding");
            assert_eq!(filter.metric, VectorMetric::InnerProduct);
            assert_eq!(filter.query_vector, vec![1.0]);
            assert_eq!(filter.op, FilterOp::Lt);
            assert_eq!(
                filter.value,
                FilterValue::Single(crate::SqlValue::Float(0.5))
            );
        }
        other => panic!("expected FilterExpr::VectorDistance, got {other:?}"),
    }
}
