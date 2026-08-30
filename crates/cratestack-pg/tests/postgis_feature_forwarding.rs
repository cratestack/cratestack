//! End-to-end check that this facade's `postgis` Cargo feature really
//! forwards all the way down: `cratestack-macros/postgis` (the
//! compile-time declaration gate — a schema declaring `extension
//! postgis { }` is a `compile_error!` without it) and
//! `cratestack-sqlx/postgis` (spatial column bind/decode). Mirrors the
//! sibling `pgvector_feature_forwarding` test.
//!
//! Gated `required-features = ["postgis"]` in `Cargo.toml`, so this
//! file only compiles under `cargo test -p cratestack-pg --features
//! postgis`. That it compiles at all is most of the assertion: the
//! whole generated surface for a spatial model — struct, Create/Update
//! inputs, row decoders, `FieldRef`s — has to typecheck.
//!
//! cratestack#842.

use cratestack::include_server_schema;

include_server_schema!(
    "tests/fixtures/postgis_feature_forwarding.cstack",
    db = Postgres
);

#[test]
fn spatial_fields_compile_as_ewkb_bytes() {
    let zone = cratestack_schema::DeliveryZone {
        id: 1,
        label: "central".to_owned(),
        // EWKB bytes — the same `Vec<u8>` shape a `Bytes` field gets.
        service_area: vec![1_u8, 2, 3],
        pickup_point: None,
    };

    assert_eq!(zone.id, 1);
    assert_eq!(zone.service_area, vec![1_u8, 2, 3]);
    assert_eq!(zone.pickup_point, None);
}

/// The #842 item-4 regression: `covers_geography`/`dwithin_geography`
/// used to require a hand-built `FieldRef` against a string column
/// name, with a fake type parameter (`Option<Vec<u8>>`) chosen only to
/// satisfy the signature — so a typo was a runtime SQL error. The
/// generated accessor makes the column name compile-checked.
#[test]
fn generated_field_ref_drives_spatial_filters() {
    use cratestack::point;

    let covers = cratestack_schema::delivery_zone::service_area().covers_geography(point(1.0, 2.0));
    let within =
        cratestack_schema::delivery_zone::service_area().dwithin_geography(point(1.0, 2.0), 500.0);

    // Distinct filter shapes, both built without naming a column string.
    assert_ne!(format!("{covers:?}"), format!("{within:?}"));
}

/// #842 item 5: `ST_Distance` ordering, the pair to `dwithin_geography`.
#[test]
fn generated_field_ref_drives_distance_ordering() {
    use cratestack::point;

    let nearest =
        cratestack_schema::delivery_zone::service_area().order_by_distance_to(point(1.0, 2.0));

    assert!(matches!(
        nearest.target,
        cratestack::OrderTarget::SpatialDistance { .. }
    ));
}
