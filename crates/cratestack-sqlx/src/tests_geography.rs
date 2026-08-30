#![cfg(test)]

use crate::{FieldRef, render::render_filter_expr_sql};

#[test]
fn covers_geography_renders_st_covers_with_two_binds() {
    let filter = FieldRef::<(), ()>::new("service_area")
        .covers_geography(cratestack_sql::point(-122.4194, 37.7749));
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_filter_expr_sql(&filter, &mut sql, &mut bind_index);
    assert_eq!(
        sql,
        "ST_Covers(service_area::geography, ST_MakePoint($1, $2)::geography)",
    );
    assert_eq!(bind_index, 3);
}

#[test]
fn dwithin_geography_renders_st_dwithin_with_three_binds() {
    let filter = FieldRef::<(), ()>::new("service_area")
        .dwithin_geography(cratestack_sql::point(-122.4194, 37.7749), 1500.0);
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_filter_expr_sql(&filter, &mut sql, &mut bind_index);
    assert_eq!(
        sql,
        "ST_DWithin(service_area::geography, ST_MakePoint($1, $2)::geography, $3)",
    );
    assert_eq!(bind_index, 4, "lng + lat + radius_meters");
}

/// cratestack#842 item 5: `ST_Distance` ordering, the pair to
/// `dwithin_geography`. Asserts the exact SQL, which was executed
/// against postgis/postgis:16-3.4 to confirm it is valid PostGIS —
/// not just a plausible-looking string.
#[test]
fn order_by_distance_to_renders_st_distance_with_two_binds() {
    use crate::render::render_order_clause_sql;

    let clause = FieldRef::<(), ()>::new("service_area")
        .order_by_distance_to(cratestack_sql::point(-122.4194, 37.7749));
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_order_clause_sql(&clause, &mut sql, &mut bind_index);
    assert_eq!(
        sql,
        "ST_Distance(service_area::geography, ST_MakePoint($1, $2)::geography) ASC NULLS LAST",
    );
    assert_eq!(bind_index, 3, "lng + lat");
}

/// `.desc()` is the farthest-first half of the same builder.
#[test]
fn distance_to_point_desc_flips_only_the_direction() {
    use crate::render::render_order_clause_sql;

    let clause = FieldRef::<(), ()>::new("service_area")
        .distance_to_point(cratestack_sql::point(1.0, 2.0))
        .desc();
    let mut bind_index = 1usize;
    let mut sql = String::new();
    render_order_clause_sql(&clause, &mut sql, &mut bind_index);
    assert!(sql.ends_with("DESC NULLS LAST"), "got: {sql}");
    assert!(
        sql.starts_with("ST_Distance(service_area::geography"),
        "got: {sql}"
    );
}
