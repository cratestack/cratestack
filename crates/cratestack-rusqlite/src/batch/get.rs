//! `BatchGet` — one-statement IN-list fetch with per-id NOT_FOUND.

use std::collections::HashMap;
use std::hash::Hash;

use cratestack_core::{BatchItemError, BatchResponse};
use cratestack_sql::{IntoSqlValue, ModelDescriptor, ModelPrimaryKey, SqlValue};
use rusqlite::params_from_iter;

use crate::{FromRusqliteRow, RusqliteError, RusqliteRuntime, SqlValueParam};

use super::support::{err_item, finalize, ok_item, reject_duplicate_pks, validate_batch_size};

pub struct BatchGet<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a RusqliteRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) ids: Vec<PK>,
}

impl<'a, M: 'static, PK: 'static> BatchGet<'a, M, PK> {
    pub fn run(self) -> Result<BatchResponse<M>, RusqliteError>
    where
        M: FromRusqliteRow + ModelPrimaryKey<PK>,
        PK: IntoSqlValue + Clone + Eq + Hash,
    {
        validate_batch_size(self.ids.len())?;
        reject_duplicate_pks(&self.ids)?;
        if self.ids.is_empty() {
            return Ok(finalize::<M>(vec![]));
        }

        // `render_select` already understands soft-delete; build an IN
        // filter manually and bind the PK list positionally.
        let mut sql = format!(
            "SELECT {} FROM {} WHERE ",
            self.descriptor.select_projection(),
            self.descriptor.table_name,
        );
        if let Some(col) = self.descriptor.soft_delete_column {
            sql.push_str(col);
            sql.push_str(" IS NULL AND ");
        }
        sql.push_str(self.descriptor.primary_key);
        sql.push_str(" IN (");
        for index in 0..self.ids.len() {
            if index > 0 {
                sql.push_str(", ");
            }
            sql.push_str(&format!("?{}", index + 1));
        }
        sql.push(')');

        let binds: Vec<SqlValue> = self
            .ids
            .iter()
            .cloned()
            .map(IntoSqlValue::into_sql_value)
            .collect();

        let rows: Vec<M> = self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let rows = stmt
                .query_map(params_from_iter(bind_iter), |row| M::from_rusqlite_row(row))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;

        let mut by_pk: HashMap<PK, M> = rows.into_iter().map(|m| (m.primary_key(), m)).collect();
        let results = self
            .ids
            .into_iter()
            .enumerate()
            .map(|(index, id)| match by_pk.remove(&id) {
                Some(record) => ok_item(index, record),
                None => err_item(
                    index,
                    BatchItemError {
                        code: "NOT_FOUND".to_owned(),
                        message: "no row matched".to_owned(),
                    },
                ),
            })
            .collect();
        Ok(finalize(results))
    }
}
