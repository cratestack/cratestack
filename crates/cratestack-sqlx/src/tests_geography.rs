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
