//! PostGIS DDL emission (cratestack#842): `CREATE EXTENSION IF NOT
//! EXISTS postgis;` plus `geography(Polygon,4326)` column rendering.
//!
//! Same shape as the sibling `extensions` module's pgvector tests —
//! positive emission is gated on the `postgis` feature (the DDL is a
//! hard panic otherwise, by design), and a negative test without the
//! feature exercises that panic directly so the gate is proven real
//! rather than dormant.

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[cfg(feature = "postgis")]
const ZONE_SCHEMA: &str = r#"
extension postgis {
}

model DeliveryZone {
  id Int @id
  serviceArea Geography(Polygon, 4326)
}
"#;

#[cfg(feature = "postgis")]
#[test]
fn create_extension_emitted_once_before_column_ddl() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(ZONE_SCHEMA));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));

    let extension_pos = migration
        .up
        .find("CREATE EXTENSION IF NOT EXISTS postgis;")
        .expect("CREATE EXTENSION should be emitted");
    let column_pos = migration
        .up
        .find("geography(Polygon,4326)")
        .expect("geography column DDL should be emitted");
    assert!(
        extension_pos < column_pos,
        "CREATE EXTENSION must precede column DDL referencing it: {}",
        migration.up
    );
    assert_eq!(
        migration
            .up
            .matches("CREATE EXTENSION IF NOT EXISTS postgis;")
            .count(),
        1,
        "CREATE EXTENSION should appear exactly once: {}",
        migration.up
    );
}

/// The core regression for #842: the reporter's column landed as TEXT
/// because no spatial type existed. It must now render as a real
/// PostGIS type and must never fall back to TEXT.
#[cfg(feature = "postgis")]
#[test]
fn geography_column_renders_postgis_type_not_text() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(ZONE_SCHEMA));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("service_area geography(Polygon,4326) NOT NULL"),
        "service_area should render as geography(Polygon,4326): {}",
        migration.up
    );
    assert!(
        !migration.up.contains("service_area TEXT"),
        "service_area must not silently fall back to TEXT: {}",
        migration.up
    );
}

#[cfg(feature = "postgis")]
#[test]
fn renders_geometry_and_optional_srid_forms() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
extension postgis {
}

model Shape {
  id Int @id
  planar Geometry(Point, 3857)
  noSrid Geometry(Point)
  bare Geography
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains("planar geometry(Point,3857)"),
        "planar column: {}",
        migration.up
    );
    assert!(
        migration.up.contains("no_srid geometry(Point)"),
        "a subtype with no SRID should defer to PostGIS's default: {}",
        migration.up
    );
    assert!(
        migration.up.contains("bare geography"),
        "an unmodified Geography is a legal PostGIS type: {}",
        migration.up
    );
}

/// Subtype casing is normalised into the IR, so re-casing a subtype in
/// `.cstack` must not read as a column-type change. Without the
/// canonicalisation in `convert::fields`, this diff would emit an
/// `ALTER COLUMN ... TYPE` for a column that didn't change.
#[cfg(feature = "postgis")]
#[test]
fn recasing_a_subtype_is_not_a_column_change() {
    let prev = schema(&with_models(ZONE_SCHEMA));
    let next = schema(&with_models(
        r#"
extension postgis {
}

model DeliveryZone {
  id Int @id
  serviceArea Geography(POLYGON, 4326)
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        !migration.up.contains("ALTER COLUMN"),
        "re-casing a subtype must not produce a column-type change: {}",
        migration.up
    );
}

/// `Geography` and `Geometry` are distinct Postgres types, not a
/// modifier on one type, so switching between them *is* a real change
/// the diff has to see.
#[cfg(feature = "postgis")]
#[test]
fn switching_geography_to_geometry_is_a_column_change() {
    let prev = schema(&with_models(ZONE_SCHEMA));
    let next = schema(&with_models(
        r#"
extension postgis {
}

model DeliveryZone {
  id Int @id
  serviceArea Geometry(Polygon, 4326)
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains("geometry(Polygon,4326)"),
        "the geography -> geometry switch must reach the DDL: {}",
        migration.up
    );
}

#[cfg(not(feature = "postgis"))]
#[test]
#[should_panic(expected = "postgis")]
fn postgis_ddl_panics_without_postgis_feature() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
extension postgis {
}

model DeliveryZone {
  id Int @id
  serviceArea Geography(Polygon, 4326)
}
"#,
    ));
    // Building `cratestack-migrate` without the `postgis` feature and
    // then feeding it a schema that declares the extension is a hard
    // panic, not silently-wrong DDL — mirrors the pgvector gate in
    // `super::extensions`.
    let _ = emit(&diff(&prev, &next).expect("diff should succeed"));
}
