//! Validation for the PostGIS scalars `Geography` / `Geometry`
//! (cratestack#842).
//!
//! Split out of `type_names.rs` rather than appended to it: that module
//! is already past the repo's ~200-LoC file ceiling, and the spatial
//! rules are self-contained.
//!
//! The accepted shapes, mirroring PostGIS's own type modifier grammar:
//!
//! ```text
//! Geography                      -> geography
//! Geography(Point)               -> geography(Point)
//! Geography(Point, 4326)         -> geography(Point,4326)
//! Geometry(MultiPolygonZ, 3857)  -> geometry(MultiPolygonZ,3857)
//! ```
//!
//! An SRID with no subtype (`Geography(4326)`) is rejected because
//! PostGIS has no such modifier — the subtype is positional and comes
//! first.

use std::collections::BTreeSet;

use cratestack_core::{ExtensionKind, SourceSpan, TypeRef, canonical_geometry_subtype};

use crate::diagnostics::{SchemaError, span_error};

/// A short, representative slice of the accepted subtype vocabulary.
/// The full list is 64 spellings (16 bases × 4 dimensionalities) —
/// too long for a diagnostic, so the message names the common 2D forms
/// and describes the suffix rule instead of enumerating it.
const COMMON_SUBTYPES: &str =
    "Point, LineString, Polygon, MultiPoint, MultiLineString, MultiPolygon, GeometryCollection";

pub(super) fn validate_spatial_type_ref(
    declared_extensions: &BTreeSet<ExtensionKind>,
    type_ref: &TypeRef,
    span: SourceSpan,
) -> Result<(), SchemaError> {
    let name = type_ref.name.as_str();

    if !declared_extensions.contains(&ExtensionKind::Postgis) {
        return Err(span_error(
            format!(
                "field type `{name}` requires `extension postgis {{ }}` to be declared in this \
                 schema — see docs/design/extensions.md §6b"
            ),
            span,
        ));
    }

    if type_ref.arity == cratestack_core::TypeArity::List {
        return Err(span_error(
            format!(
                "`{name}` fields cannot be list-valued (`{name}[]`) — PostGIS has no array-of-\
                 geography column type; use a `MultiPoint`/`MultiPolygon` subtype to hold several \
                 shapes in one value"
            ),
            span,
        ));
    }

    if type_ref.int_args.len() > 1 {
        return Err(span_error(
            format!(
                "`{name}` accepts at most one integer SRID argument, e.g. `{name}(Point, 4326)`"
            ),
            span,
        ));
    }

    if type_ref.ident_args.len() > 1 {
        return Err(span_error(
            format!(
                "`{name}` accepts at most one geometry subtype argument, e.g. \
                 `{name}(Point, 4326)`"
            ),
            span,
        ));
    }

    // PostGIS's modifier is positional: the subtype comes first, so an
    // SRID can't be given on its own.
    if type_ref.ident_args.is_empty() && !type_ref.int_args.is_empty() {
        return Err(span_error(
            format!(
                "`{name}` cannot take an SRID without a geometry subtype — write \
                 `{name}(Point, {srid})` rather than `{name}({srid})`",
                srid = type_ref.int_args[0]
            ),
            span,
        ));
    }

    if let Some(subtype) = type_ref.ident_args.first()
        && canonical_geometry_subtype(subtype).is_none()
    {
        return Err(span_error(
            format!(
                "unknown geometry subtype `{subtype}` in `{name}({subtype})` — expected one of \
                 {COMMON_SUBTYPES}, optionally suffixed `Z`, `M`, or `ZM` for 3D/measured \
                 geometries (e.g. `PointZM`)"
            ),
            span,
        ));
    }

    Ok(())
}
