//! The PostGIS geometry vocabulary shared by the parser (which
//! validates what a schema may write) and the migrate emitter (which
//! renders the Postgres type modifier). Keeping the closed set here
//! rather than in either crate means the two can never drift into
//! accepting and emitting different spellings.
//!
//! See `docs/design/extensions.md` §6b and cratestack#842.

/// Base PostGIS geometry subtype names, in the canonical casing
/// PostGIS itself reports from `geometry_columns`. A schema may write
/// any casing (`POINT`, `point`, `Point`) — [`canonical_geometry_subtype`]
/// normalises to these spellings so the emitted DDL and the migration
/// snapshot agree byte-for-byte regardless of how the field was typed.
///
/// `Geometry` is included as a subtype in its own right: PostGIS
/// accepts `geography(Geometry, 4326)` as the "any subtype" modifier,
/// which is distinct from an unmodified `geography` column.
const BASE_SUBTYPES: &[&str] = &[
    "Geometry",
    "Point",
    "LineString",
    "Polygon",
    "MultiPoint",
    "MultiLineString",
    "MultiPolygon",
    "GeometryCollection",
    "CircularString",
    "CompoundCurve",
    "CurvePolygon",
    "MultiCurve",
    "MultiSurface",
    "PolyhedralSurface",
    "Triangle",
    "Tin",
];

/// Dimensionality suffixes PostGIS allows on any base subtype —
/// `PointZ` (3D), `PointM` (measured), `PointZM` (both). The empty
/// suffix is the ordinary 2D form.
const DIMENSION_SUFFIXES: &[&str] = &["", "Z", "M", "ZM"];

/// Normalises a schema-written geometry subtype to its canonical
/// PostGIS casing, or `None` if it isn't a recognised subtype.
///
/// Matching is case-insensitive on both the base name and the
/// dimensionality suffix, because PostGIS's own type modifier parser
/// is. Rejecting an unrecognised name is the entire point of making
/// the column declarable: before cratestack#842 a typo'd spatial
/// column was simply not expressible, and the workaround (an
/// undeclared column plus a hand-written migration) pushed the typo
/// to a runtime SQL error.
pub fn canonical_geometry_subtype(name: &str) -> Option<&'static str> {
    // Longest suffix first, so `PointZM` matches `ZM` rather than
    // stripping `M` and failing to resolve `PointZ` as a base name.
    let mut suffixes: Vec<&&str> = DIMENSION_SUFFIXES.iter().collect();
    suffixes.sort_by_key(|suffix| std::cmp::Reverse(suffix.len()));

    for suffix in suffixes {
        let Some(base) = strip_suffix_ignore_ascii_case(name, suffix) else {
            continue;
        };
        for candidate in BASE_SUBTYPES {
            if candidate.eq_ignore_ascii_case(base) {
                return canonical_spelling(candidate, suffix);
            }
        }
    }
    None
}

/// Every accepted subtype spelling, for building "expected one of: …"
/// diagnostics. Ordered base-major so the common 2D names lead.
pub fn geometry_subtype_names() -> Vec<String> {
    DIMENSION_SUFFIXES
        .iter()
        .flat_map(|suffix| {
            BASE_SUBTYPES
                .iter()
                .map(move |base| format!("{base}{suffix}"))
        })
        .collect()
}

/// Resolves a `(base, suffix)` pair back to a `&'static str`.
///
/// The canonical spellings are enumerated at compile time rather than
/// formatted, so callers get a `&'static str` they can embed in
/// generated code and IR without an allocation. Returns `None` only
/// for a pair this module never produced.
fn canonical_spelling(base: &'static str, suffix: &str) -> Option<&'static str> {
    if suffix.is_empty() {
        return Some(base);
    }
    CANONICAL_SUFFIXED
        .iter()
        .find(|candidate| {
            candidate.len() == base.len() + suffix.len()
                && candidate.starts_with(base)
                && candidate[base.len()..].eq_ignore_ascii_case(suffix)
        })
        .copied()
}

/// The `Z`/`M`/`ZM` spellings of every base subtype, so
/// [`canonical_spelling`] can hand back `&'static str`.
const CANONICAL_SUFFIXED: &[&str] = &[
    "GeometryZ",
    "GeometryM",
    "GeometryZM",
    "PointZ",
    "PointM",
    "PointZM",
    "LineStringZ",
    "LineStringM",
    "LineStringZM",
    "PolygonZ",
    "PolygonM",
    "PolygonZM",
    "MultiPointZ",
    "MultiPointM",
    "MultiPointZM",
    "MultiLineStringZ",
    "MultiLineStringM",
    "MultiLineStringZM",
    "MultiPolygonZ",
    "MultiPolygonM",
    "MultiPolygonZM",
    "GeometryCollectionZ",
    "GeometryCollectionM",
    "GeometryCollectionZM",
    "CircularStringZ",
    "CircularStringM",
    "CircularStringZM",
    "CompoundCurveZ",
    "CompoundCurveM",
    "CompoundCurveZM",
    "CurvePolygonZ",
    "CurvePolygonM",
    "CurvePolygonZM",
    "MultiCurveZ",
    "MultiCurveM",
    "MultiCurveZM",
    "MultiSurfaceZ",
    "MultiSurfaceM",
    "MultiSurfaceZM",
    "PolyhedralSurfaceZ",
    "PolyhedralSurfaceM",
    "PolyhedralSurfaceZM",
    "TriangleZ",
    "TriangleM",
    "TriangleZM",
    "TinZ",
    "TinM",
    "TinZM",
];

fn strip_suffix_ignore_ascii_case<'a>(value: &'a str, suffix: &str) -> Option<&'a str> {
    if suffix.is_empty() {
        return Some(value);
    }
    let split = value.len().checked_sub(suffix.len())?;
    let (base, tail) = value.split_at(split);
    (tail.eq_ignore_ascii_case(suffix) && !base.is_empty()).then_some(base)
}

#[cfg(test)]
mod tests;
