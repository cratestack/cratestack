use crate::order::{OrderClause, OrderTarget};
use crate::{NullOrder, SortDirection};

/// PostGIS spatial filter primitives. v1 ships two ops that cover the
/// "is point inside this zone" / "is this point within radius of that
/// zone" cases — the rest of the ST_* surface can land on demand.
///
/// All current variants treat the column as `geography(Point, 4326)`
/// — the WGS-84 lat/lon CRS that PostGIS ships with extensions
/// enabled. Casting happens at render time via `::geography` so
/// schemas storing `geometry` or `geometry(...)` columns work without
/// extra annotations, at the cost of an in-flight cast each call.
#[derive(Debug, Clone, PartialEq)]
pub enum SpatialFilter {
    /// `ST_Covers(col::geography, ST_MakePoint($lng, $lat)::geography)`.
    /// Matches when the column's geography fully covers (contains
    /// including the boundary) the given point. Use for "is this
    /// caller-supplied point inside the row's service area" lookups.
    CoversGeographyPoint {
        column: &'static str,
        lng: f64,
        lat: f64,
    },
    /// `ST_DWithin(col::geography, ST_MakePoint($lng, $lat)::geography,
    /// $radius_meters)`. Matches when the column's geography is
    /// within `radius_meters` of the given point (great-circle
    /// distance on WGS-84, since `::geography` triggers the spheroid
    /// path).
    DWithinGeographyPoint {
        column: &'static str,
        lng: f64,
        lat: f64,
        radius_meters: f64,
    },
}

/// Builder returned by [`crate::point`] for assembling a spatial
/// filter. Holds nothing but the lat/lng pair until a comparator is
/// chained.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpatialPoint {
    pub lng: f64,
    pub lat: f64,
}

/// Geographic point (WGS-84 lng/lat). The naming follows the PostGIS
/// `ST_MakePoint(x, y)` convention — `lng` is the X axis (longitude),
/// `lat` is the Y axis (latitude). Don't accidentally swap them;
/// the engine has no way to detect it and your filter will silently
/// match points across the world.
pub const fn point(lng: f64, lat: f64) -> SpatialPoint {
    SpatialPoint { lng, lat }
}

/// Builder returned by `FieldRef::distance_to_point` — chain `.asc()`
/// or `.desc()` to turn great-circle distance into an `ORDER BY`
/// target (cratestack#842 item 5). Mirrors
/// [`crate::VectorDistanceExpr`]'s ordering half.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpatialDistanceExpr {
    column: &'static str,
    point: SpatialPoint,
}

impl SpatialDistanceExpr {
    pub(super) const fn new(column: &'static str, point: SpatialPoint) -> Self {
        Self { column, point }
    }

    /// Nearest first — the common "closest depot to this address" shape.
    pub fn asc(self) -> OrderClause {
        self.order(SortDirection::Asc)
    }

    /// Farthest first.
    pub fn desc(self) -> OrderClause {
        self.order(SortDirection::Desc)
    }

    fn order(self, direction: SortDirection) -> OrderClause {
        OrderClause {
            target: OrderTarget::SpatialDistance {
                column: self.column,
                lng: self.point.lng,
                lat: self.point.lat,
            },
            direction,
            null_order: NullOrder::Last,
        }
    }
}
