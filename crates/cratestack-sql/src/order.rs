#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderTarget {
    Column(&'static str),
    RelationScalar {
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        value_sql: &'static str,
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

    pub const fn relation_scalar(
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        value_sql: &'static str,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}
