//! `Aggregate` entry — hands out per-op builders (count/sum/avg/min/max)
//! and owns the shared rendering primitive.

use std::fmt::Write;

use cratestack_sql::{FilterExpr, ModelDescriptor, SqlValue, SqliteDialect};

use crate::RusqliteRuntime;

use super::aggregate_column::AggregateColumn;
use super::aggregate_count::AggregateCount;

pub struct Aggregate<'a, M: 'static, PK: 'static> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static ModelDescriptor<M, PK>,
}

impl<'a, M: 'static, PK: 'static> Aggregate<'a, M, PK> {
    pub fn count(self) -> AggregateCount<'a, M, PK> {
        AggregateCount {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
        }
    }

    pub fn sum<C: cratestack_sql::IntoColumnName>(self, column: C) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Sum, column)
    }

    pub fn avg<C: cratestack_sql::IntoColumnName>(self, column: C) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Avg, column)
    }

    pub fn min<C: cratestack_sql::IntoColumnName>(self, column: C) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Min, column)
    }

    pub fn max<C: cratestack_sql::IntoColumnName>(self, column: C) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Max, column)
    }
}

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

pub(super) enum AggregateProjection<'a> {
    CountStar,
    Column {
        function: &'static str,
        column: &'a str,
    },
}

pub(super) fn render_aggregate<M, PK>(
    descriptor: &ModelDescriptor<M, PK>,
    projection: AggregateProjection<'_>,
    filters: &[FilterExpr],
) -> (String, Vec<SqlValue>) {
    let dialect = SqliteDialect;
    let mut sql = String::from("SELECT ");
    match projection {
        AggregateProjection::CountStar => sql.push_str("COUNT(*)"),
        AggregateProjection::Column { function, column } => {
            let _ = write!(sql, "{function}({column})");
        }
    }
    let _ = write!(sql, " FROM {}", descriptor.table_name);

    let mut binds: Vec<SqlValue> = Vec::new();
    let mut bind_index = 1usize;
    let mut where_started = false;
    if let Some(soft_delete) = descriptor.soft_delete_column {
        let _ = write!(sql, " WHERE {soft_delete} IS NULL");
        where_started = true;
    }
    if !filters.is_empty() {
        sql.push_str(if where_started { " AND " } else { " WHERE " });
        let mut joined = false;
        for filter in filters {
            if joined {
                sql.push_str(" AND ");
            }
            crate::render::render_filter_expr(
                &dialect,
                filter,
                &mut sql,
                &mut binds,
                &mut bind_index,
            );
            joined = true;
        }
    }
    (sql, binds)
}
