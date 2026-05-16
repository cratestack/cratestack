//! `Aggregate` entry point — factory that hands out per-op builders.
//! Like `find_many`, aggregates are filtered through the model's read
//! policy AND the soft-delete column so counts/sums describe rows the
//! caller would be allowed to retrieve.

use cratestack_sql::IntoColumnName;

use crate::{ModelDescriptor, SqlxRuntime};

use super::aggregate_column::AggregateColumn;
use super::aggregate_count::AggregateCount;

#[derive(Debug, Clone, Copy)]
pub(super) enum AggregateOp {
    Sum,
    Avg,
    Min,
    Max,
}

impl AggregateOp {
    pub(super) fn function_name(self) -> &'static str {
        match self {
            Self::Sum => "SUM",
            Self::Avg => "AVG",
            Self::Min => "MIN",
            Self::Max => "MAX",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Aggregate<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
}

impl<'a, M: 'static, PK: 'static> Aggregate<'a, M, PK> {
    /// `COUNT(*)` over the matching rows. Always returns `i64`; empty
    /// matches yield 0 rather than NULL.
    pub fn count(self) -> AggregateCount<'a, M, PK> {
        AggregateCount::new(self.runtime, self.descriptor)
    }

    /// `SUM(<col>)`. Returns `Option<T>` — `None` when no rows match.
    pub fn sum<C: IntoColumnName>(self, column: C) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Sum, column)
    }

    /// `AVG(<col>)`. Returns `Option<T>` — `None` when no rows match.
    pub fn avg<C: IntoColumnName>(self, column: C) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Avg, column)
    }

    /// `MIN(<col>)`. Returns `Option<T>` — `None` when no rows match.
    pub fn min<C: IntoColumnName>(self, column: C) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Min, column)
    }

    /// `MAX(<col>)`. Returns `Option<T>` — `None` when no rows match.
    pub fn max<C: IntoColumnName>(self, column: C) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Max, column)
    }
}
