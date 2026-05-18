//! `ProjectedFindUnique` — `.select(...)` on a `FindUnique` produces a
//! partial-row `Projection<M>` instead of a full model.

use cratestack_sql::{IntoSqlValue, ReadSource};
use rusqlite::params_from_iter;

use crate::{RusqliteError, RusqliteRuntime, SqlValueParam};

pub struct ProjectedFindUnique<'a, M: 'static, PK: 'static> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static dyn ReadSource<M, PK>,
    pub(super) id: PK,
    pub(super) selected: Vec<&'static str>,
}

impl<'a, M: 'static, PK: 'static> ProjectedFindUnique<'a, M, PK>
where
    PK: IntoSqlValue + Clone,
{
    pub fn run(self) -> Result<Option<cratestack_sql::Projection<M>>, RusqliteError>
    where
        M: crate::FromPartialRusqliteRow,
    {
        let projection_sql = self.descriptor.select_projection_subset(&self.selected);
        let mut sql = format!(
            "SELECT {} FROM {} WHERE {} = ?1",
            projection_sql, self.descriptor.table_name(), self.descriptor.primary_key(),
        );
        if let Some(soft_delete) = self.descriptor.soft_delete_column() {
            sql.push_str(&format!(" AND {soft_delete} IS NULL"));
        }
        sql.push_str(" LIMIT 1");
        let bind = self.id.clone().into_sql_value();
        let selected = self.selected;
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = std::iter::once(SqlValueParam(&bind));
            let mut rows = stmt.query(params_from_iter(bind_iter))?;
            if let Some(row) = rows.next()? {
                let value = M::from_partial_rusqlite_row(row, &selected)?;
                Ok(Some(cratestack_sql::Projection { value, selected }))
            } else {
                Ok(None)
            }
        })
    }

    pub fn run_in_tx(
        self,
        conn: &rusqlite::Connection,
    ) -> Result<Option<cratestack_sql::Projection<M>>, RusqliteError>
    where
        M: crate::FromPartialRusqliteRow,
    {
        let projection_sql = self.descriptor.select_projection_subset(&self.selected);
        let mut sql = format!(
            "SELECT {} FROM {} WHERE {} = ?1",
            projection_sql, self.descriptor.table_name(), self.descriptor.primary_key(),
        );
        if let Some(soft_delete) = self.descriptor.soft_delete_column() {
            sql.push_str(&format!(" AND {soft_delete} IS NULL"));
        }
        sql.push_str(" LIMIT 1");
        let bind = self.id.clone().into_sql_value();
        let mut stmt = conn.prepare(&sql)?;
        let bind_iter = std::iter::once(SqlValueParam(&bind));
        let mut rows = stmt.query(params_from_iter(bind_iter))?;
        if let Some(row) = rows.next()? {
            let value = M::from_partial_rusqlite_row(row, &self.selected)?;
            Ok(Some(cratestack_sql::Projection {
                value,
                selected: self.selected,
            }))
        } else {
            Ok(None)
        }
    }
}
