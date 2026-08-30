use super::{canonical_geometry_subtype, geometry_subtype_names};

#[test]
fn canonicalises_case_insensitively() {
    for written in ["Point", "point", "POINT", "PoInT"] {
        assert_eq!(
            canonical_geometry_subtype(written),
            Some("Point"),
            "`{written}` should normalise to the canonical `Point`"
        );
    }
}

#[test]
fn accepts_every_base_subtype() {
    for name in [
        "Geometry",
        "LineString",
        "Polygon",
        "MultiPoint",
        "MultiLineString",
        "MultiPolygon",
        "GeometryCollection",
        "Triangle",
        "Tin",
    ] {
        assert_eq!(canonical_geometry_subtype(name), Some(name));
    }
}

#[test]
fn accepts_dimensionality_suffixes() {
    assert_eq!(canonical_geometry_subtype("PointZ"), Some("PointZ"));
    assert_eq!(canonical_geometry_subtype("PointM"), Some("PointM"));
    assert_eq!(canonical_geometry_subtype("PointZM"), Some("PointZM"));
    assert_eq!(canonical_geometry_subtype("pointzm"), Some("PointZM"));
    assert_eq!(
        canonical_geometry_subtype("multipolygonzm"),
        Some("MultiPolygonZM")
    );
}

/// `PointZM` must not be resolved by stripping the shorter `M`
/// suffix and then failing to find a `PointZ` *base* — the longest
/// suffix has to win. Guards the ordering in `canonical_geometry_subtype`.
#[test]
fn prefers_the_longest_dimensionality_suffix() {
    assert_eq!(canonical_geometry_subtype("PointZM"), Some("PointZM"));
    assert_eq!(
        canonical_geometry_subtype("GeometryCollectionZM"),
        Some("GeometryCollectionZM")
    );
}

#[test]
fn rejects_unknown_subtypes() {
    for name in ["Pointt", "Polygone", "Circle", "", "Z", "ZM", "Blob"] {
        assert_eq!(
            canonical_geometry_subtype(name),
            None,
            "`{name}` is not a PostGIS subtype and must be rejected"
        );
    }
}

/// The diagnostic list and the acceptor must agree: anything the
/// error message advertises as valid has to actually validate.
#[test]
fn every_advertised_name_is_accepted() {
    let names = geometry_subtype_names();
    assert!(!names.is_empty());
    for name in names {
        assert_eq!(
            canonical_geometry_subtype(&name),
            Some(name.as_str()),
            "`{name}` is advertised in diagnostics but not accepted"
        );
    }
}
