#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderClause {
    pub target: OrderTarget,
    pub direction: SortDirection,
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
        }
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
