//! `Geography` / `Geometry` scalar parsing and validation
//! (cratestack#842).

use crate::parse_schema;

fn schema_with(extension: &str, field: &str) -> String {
    format!(
        r#"
datasource db {{
  provider = "postgresql"
  url = env("DATABASE_URL")
}}

{extension}

model DeliveryZone {{
  id Int @id
  {field}
}}
"#
    )
}

const POSTGIS: &str = "extension postgis {\n}";

#[test]
fn parses_subtype_and_srid() {
    let schema = parse_schema(&schema_with(
        POSTGIS,
        "serviceArea Geography(Polygon, 4326)",
    ))
    .expect("`Geography(Polygon, 4326)` should parse with `extension postgis { }` declared");

    let field = &schema.models[0].fields[1];
    assert_eq!(field.ty.name, "Geography");
    assert_eq!(field.ty.spatial_subtype(), Some("Polygon"));
    assert_eq!(field.ty.spatial_srid(), Some(4326));
    assert!(field.ty.is_geography());
}

#[test]
fn parses_subtype_without_srid() {
    let schema = parse_schema(&schema_with(POSTGIS, "pickupPoint Geometry(Point)"))
        .expect("the one-argument form should parse");

    let field = &schema.models[0].fields[1];
    assert_eq!(field.ty.spatial_subtype(), Some("Point"));
    assert_eq!(
        field.ty.spatial_srid(),
        None,
        "no SRID written means defer to PostGIS's own default, not an invented one"
    );
    assert!(
        !field.ty.is_geography(),
        "`Geometry` is planar, not spheroidal"
    );
}

/// The exact field the #842 reporter wrote. Before this change it was
/// rejected outright (`unknown type `Geography``), which is what forced
/// the duplicate-input-column workaround.
#[test]
fn parses_bare_geography() {
    let schema = parse_schema(&schema_with(POSTGIS, "serviceArea Geography"))
        .expect("bare `Geography` is a legal PostGIS column type and should parse");

    let field = &schema.models[0].fields[1];
    assert_eq!(field.ty.spatial_subtype(), None);
    assert_eq!(field.ty.spatial_srid(), None);
}

#[test]
fn optional_arity_is_allowed() {
    parse_schema(&schema_with(
        POSTGIS,
        "serviceArea Geography(Polygon, 4326)?",
    ))
    .expect("an optional spatial column should parse");
}

#[test]
fn requires_the_postgis_extension() {
    let error = parse_schema(&schema_with("", "serviceArea Geography(Polygon, 4326)"))
        .expect_err("`Geography` without `extension postgis { }` should fail validation");
    assert!(
        error.to_string().contains("extension postgis"),
        "error should name the missing extension, got: {error}"
    );
}

#[test]
fn rejects_list_arity() {
    let error = parse_schema(&schema_with(POSTGIS, "areas Geography(Polygon, 4326)[]"))
        .expect_err("PostGIS has no array-of-geography column type");
    assert!(
        error.to_string().contains("cannot be list-valued"),
        "error should explain the list restriction, got: {error}"
    );
}

/// The whole point of making the column declarable: a typo becomes a
/// schema error instead of a runtime SQL error.
#[test]
fn rejects_unknown_subtype() {
    let error = parse_schema(&schema_with(
        POSTGIS,
        "serviceArea Geography(Polygone, 4326)",
    ))
    .expect_err("`Polygone` is a typo and must not reach the emitter");
    assert!(
        error.to_string().contains("unknown geometry subtype"),
        "error should flag the subtype, got: {error}"
    );
}

/// PostGIS's modifier is positional — `geography(4326)` is not valid
/// DDL, so it must not be expressible in a schema either.
#[test]
fn rejects_srid_without_subtype() {
    let error = parse_schema(&schema_with(POSTGIS, "serviceArea Geography(4326)"))
        .expect_err("an SRID with no subtype should be rejected");
    assert!(
        error.to_string().contains("without a geometry subtype"),
        "error should explain the positional rule, got: {error}"
    );
}

#[test]
fn rejects_extra_arguments() {
    parse_schema(&schema_with(
        POSTGIS,
        "serviceArea Geography(Polygon, 4326, 7)",
    ))
    .expect_err("a second SRID argument should be rejected");
    parse_schema(&schema_with(
        POSTGIS,
        "serviceArea Geography(Polygon, Point)",
    ))
    .expect_err("a second subtype argument should be rejected");
}

#[test]
fn rejects_spatial_in_procedure_signatures() {
    let source = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

extension postgis {
}

procedure locate(area: Geography(Polygon, 4326)): Int
"#;
    let error =
        parse_schema(source).expect_err("spatial types are model-field-only in this release");
    assert!(
        error.to_string().contains("model/mixin/type/auth fields"),
        "error should explain where spatial types are allowed, got: {error}"
    );
}

/// Guards the grammar generalisation: widening the parenthesized
/// argument list to accept identifiers must not let a non-parametric
/// type silently carry one.
#[test]
fn non_parametric_types_still_reject_arguments() {
    let error = parse_schema(&schema_with(POSTGIS, "label String(Point)"))
        .expect_err("`String` takes no parametric argument");
    assert!(
        error
            .to_string()
            .contains("does not accept a parametric argument"),
        "error should reject the argument, got: {error}"
    );
}

/// Subtype casing is normalised by the emitter, but the parser must
/// accept whatever PostGIS itself would.
#[test]
fn accepts_any_subtype_casing() {
    for written in ["point", "POINT", "Point", "pointzm"] {
        parse_schema(&schema_with(
            POSTGIS,
            &format!("pickupPoint Geography({written}, 4326)"),
        ))
        .unwrap_or_else(|error| panic!("`{written}` should parse, got: {error}"));
    }
}
