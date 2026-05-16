use super::expr::FilterExpr;
use super::field_ref::FieldRef;
use super::json::{JsonFilter, JsonTextPath};
use super::spatial::{SpatialFilter, SpatialPoint};

impl<M, T> FieldRef<M, T> {
    /// PG: `col ? 'key'` — the JSON document contains `key` as a
    /// top-level field. SQLite (no native `?` operator): lowers to
    /// `json_extract(col, '$.key') IS NOT NULL`.
    ///
    /// Intended for `jsonb` / JSON columns. Using this on a non-JSON
    /// column compiles fine but errors at the engine layer when the
    /// SQL runs — Rust's type system doesn't gate this for you.
    ///
    /// The key is taken as `impl Into<String>` so callers can pass
    /// either a `&'static str` literal or a runtime-owned `String`
    /// (e.g. user-driven analytics queries that pivot on a metric
    /// name from the request).
    pub fn json_has_key(self, key: impl Into<String>) -> FilterExpr {
        FilterExpr::Json(JsonFilter::HasKey {
            column: self.column,
            key: key.into(),
        })
    }

    /// PG: `col ->> 'key' <op> $1` — extract the value at `key` as
    /// text, then compare. SQLite: `json_extract(col, '$.key') <op>
    /// $1`. Returns a [`JsonTextPath`] that supports the standard
    /// comparison ops via chained methods. See [`Self::json_has_key`]
    /// for the key-ownership rationale.
    pub fn json_get_text(self, key: impl Into<String>) -> JsonTextPath {
        JsonTextPath::new(self.column, key.into())
    }

    /// PG-only: `ST_Covers(col::geography, point::geography)` — the
    /// column's geography contains `point` (including boundary).
    /// Use for "is this caller-supplied point inside the row's
    /// service area" filters on `geography(Polygon, 4326)` columns.
    ///
    /// The embedded rusqlite backend doesn't ship SpatiaLite, so
    /// this filter fails loud at the render layer there. Document at
    /// the schema level whether a model supports the embedded
    /// backend at all before using spatial ops on it.
    pub fn covers_geography(self, point: SpatialPoint) -> FilterExpr {
        FilterExpr::Spatial(SpatialFilter::CoversGeographyPoint {
            column: self.column,
            lng: point.lng,
            lat: point.lat,
        })
    }

    /// PG-only: `ST_DWithin(col::geography, point::geography,
    /// radius_meters)` — the column's geography is within
    /// `radius_meters` of the given point (great-circle distance,
    /// since `::geography` triggers the spheroid path).
    pub fn dwithin_geography(self, point: SpatialPoint, radius_meters: f64) -> FilterExpr {
        FilterExpr::Spatial(SpatialFilter::DWithinGeographyPoint {
            column: self.column,
            lng: point.lng,
            lat: point.lat,
            radius_meters,
        })
    }
}
