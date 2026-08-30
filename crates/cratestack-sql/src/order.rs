use crate::filter::VectorMetric;

// Not `Eq`: `OrderTarget::VectorDistance` carries a `Vec<f32>` query
// vector, and `f32` has no sound total-equality impl (NaN != NaN) —
// same reason `FilterExpr`/`SpatialFilter` stop at `PartialEq`.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderClause {
    pub target: OrderTarget,
    pub direction: SortDirection,
    pub null_order: NullOrder,
}

/// Where NULLs sort relative to non-NULL values. PostgreSQL's default is
/// `NULLS LAST` for `ASC` and `NULLS FIRST` for `DESC`; SQLite's default
/// is `NULLS FIRST` for both. CrateStack pins the framework default to
/// `NULLS LAST` so listings stay deterministic across backends and so
/// soft-deleted rows (typed `Option<DateTime>` that surface as `None`
/// for visible rows) don't muscle their way to the top of every listing.
/// Override per-clause via [`OrderClause::nulls_first`] when scheduler /
/// outbox queries want fresh-as-null tasks at the head of the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NullOrder {
    First,
    #[default]
    Last,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderTarget {
    Column(&'static str),
    RelationScalar {
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        /// Owned rather than `&'static str`: the correlated-subquery chain
        /// is folded from the traversed relation path at call time (see
        /// [`crate::order_value_sql`]). It used to be baked in per path at
        /// macro-expansion time, which is exactly what made codegen
        /// exponential in relation-graph connectivity (cratestack#252).
        value_sql: String,
    },
    /// Order by distance to a query vector on a `Vector(n)` column (see
    /// `docs/design/extensions.md` §6/§7, cratestack#163). Built via
    /// `FieldRef::distance_to(...).asc()`/`.desc()` or the
    /// `order_by_distance` shorthand. PG-only (pgvector) — the
    /// embedded rusqlite backend doesn't ship pgvector, so its
    /// renderer fails loud, mirroring how `FilterExpr::Spatial` is
    /// handled there.
    VectorDistance {
        column: &'static str,
        metric: VectorMetric,
        query_vector: Vec<f32>,
    },
    /// Order by `ST_Distance(col::geography, point::geography)` — great-
    /// circle metres to a reference point (cratestack#842 item 5).
    /// Built via `FieldRef::order_by_distance_to(point)`.
    ///
    /// This is the ordering half of the pair whose filtering half is
    /// [`crate::SpatialFilter::DWithinGeographyPoint`]: `DWithin` picks
    /// the rows inside a radius, this sorts them nearest-first, so
    /// "closest N within X metres" no longer needs the distance
    /// recomputed in application code after the radius filter returns.
    ///
    /// PG-only (PostGIS) — the embedded rusqlite backend doesn't ship
    /// SpatiaLite, so its renderer fails loud, exactly as it does for
    /// `FilterExpr::Spatial`.
    #[cfg(feature = "postgis")]
    SpatialDistance {
        column: &'static str,
        lng: f64,
        lat: f64,
    },
}

impl OrderClause {
    pub const fn column(column: &'static str, direction: SortDirection) -> Self {
        Self {
            target: OrderTarget::Column(column),
            direction,
            null_order: NullOrder::Last,
        }
    }

    /// Not `const` (unlike [`OrderClause::column`]): `value_sql` is folded
    /// from the traversed relation path at call time rather than baked in
    /// at macro-expansion time. See [`crate::order_value_sql`].
    pub fn relation_scalar(
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        value_sql: String,
        direction: SortDirection,
    ) -> Self {
        Self {
            target: OrderTarget::RelationScalar {
                parent_table,
                parent_column,
                related_table,
                related_column,
                value_sql,
            },
            direction,
            null_order: NullOrder::Last,
        }
    }

    /// Place NULL values *before* non-NULL ones for this clause. Use on
    /// scheduler / outbox listings where "no scheduled time yet" should
    /// sort ahead of every retry-scheduled row.
    pub fn nulls_first(mut self) -> Self {
        self.null_order = NullOrder::First;
        self
    }

    /// Place NULL values *after* non-NULL ones (the framework default).
    /// Mostly useful when overriding a programmatically-built clause
    /// that previously asked for `nulls_first`.
    pub fn nulls_last(mut self) -> Self {
        self.null_order = NullOrder::Last;
        self
    }

    pub fn is_relation_scalar(&self) -> bool {
        matches!(self.target, OrderTarget::RelationScalar { .. })
    }

    pub fn targets_column(&self, column: &str) -> bool {
        matches!(self.target, OrderTarget::Column(candidate) if candidate == column)
    }

    pub fn direction(&self) -> SortDirection {
        self.direction
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortDirection {
    Asc,
    Desc,
}
