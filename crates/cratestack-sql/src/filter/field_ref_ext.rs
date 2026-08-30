use crate::OrderClause;

use super::expr::FilterExpr;
use super::field_ref::FieldRef;
use super::json::{JsonFilter, JsonTextPath};
use super::spatial::{SpatialDistanceExpr, SpatialFilter, SpatialPoint};
use super::vector::{VectorDistanceExpr, VectorMetric};

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

    /// PG-only: `ORDER BY ST_Distance(col::geography, point::geography)`
    /// — nearest first (cratestack#842 item 5). Pairs with
    /// [`Self::dwithin_geography`]: filter to a radius, then sort by
    /// true great-circle distance inside it, rather than re-deriving
    /// the distance in application code after the rows come back.
    ///
    /// `NULL` geographies compare as `NULL` distance, which sorts last
    /// under the framework's default [`crate::NullOrder::Last`].
    pub fn order_by_distance_to(self, point: SpatialPoint) -> OrderClause {
        self.distance_to_point(point).asc()
    }

    /// Distance-to-a-point as an orderable target — chain `.asc()`
    /// (nearest first) or `.desc()` (farthest first). Use
    /// [`Self::order_by_distance_to`] for the common nearest-first case.
    pub fn distance_to_point(self, point: SpatialPoint) -> SpatialDistanceExpr {
        SpatialDistanceExpr::new(self.column, point)
    }
}

impl<M> FieldRef<M, Vec<f32>> {
    /// Left-hand operand of a `Vector(n)` distance comparison (see
    /// `docs/design/extensions.md` §6/§7, cratestack#163) — chain a
    /// comparator (`.lt`/`.lte`/`.gt`/`.gte`/`.eq`) for a threshold
    /// filter, or `.asc`/`.desc` to use it as an `ORDER BY` target. The
    /// metric is never inferred from an index (a similarity search
    /// must keep working with no vector index present, per AC #2 on
    /// cratestack#163) — pass it explicitly, or derive it from a known
    /// index opclass via [`VectorMetric::from_opclass`].
    ///
    /// PG-only (pgvector); the embedded rusqlite backend fails loud at
    /// render time if reached, mirroring [`Self::covers_geography`].
    pub fn distance_to(self, metric: VectorMetric, query_vector: Vec<f32>) -> VectorDistanceExpr {
        VectorDistanceExpr::new(self.column, metric, query_vector)
    }

    /// Shorthand for `distance_to(metric, query_vector).asc()` — order
    /// by distance, nearest first, the standard k-NN similarity-search
    /// shape.
    pub fn order_by_distance(self, metric: VectorMetric, query_vector: Vec<f32>) -> OrderClause {
        self.distance_to(metric, query_vector).asc()
    }
}

impl<M> FieldRef<M, Option<Vec<f32>>> {
    /// Same as [`FieldRef::<M, Vec<f32>>::distance_to`] for a nullable
    /// `Vector(n)` column. Rows with a `NULL` vector compare as `NULL`
    /// distance, which sorts last under the framework's default
    /// `NULLS LAST` ordering ([`crate::NullOrder`]) and never matches a
    /// threshold filter.
    pub fn distance_to(self, metric: VectorMetric, query_vector: Vec<f32>) -> VectorDistanceExpr {
        VectorDistanceExpr::new(self.column, metric, query_vector)
    }

    /// Shorthand for `distance_to(metric, query_vector).asc()`.
    pub fn order_by_distance(self, metric: VectorMetric, query_vector: Vec<f32>) -> OrderClause {
        self.distance_to(metric, query_vector).asc()
    }
}
