use crate::{IntoSqlValue, SqlValue, values::FilterValue};

use super::op::FilterOp;

#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub column: &'static str,
    pub op: FilterOp,
    pub value: FilterValue,
}

impl Filter {
    pub(super) fn single<V>(column: &'static str, op: FilterOp, value: V) -> Self
    where
        V: IntoSqlValue,
    {
        Self {
            column,
            op,
            value: FilterValue::Single(value.into_sql_value()),
        }
    }

    pub(super) fn string_pattern(
        column: &'static str,
        op: FilterOp,
        pattern: &str,
        value: impl Into<String>,
    ) -> Self {
        Self {
            column,
            op,
            value: FilterValue::Single(SqlValue::String(pattern.replacen("{}", &value.into(), 1))),
        }
    }
}
