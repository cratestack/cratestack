//! `BatchUpdate` — per-item SAVEPOINT UPDATE inside one outer transaction.

use std::hash::Hash;

use cratestack_core::{BatchItemError, BatchResponse};
use cratestack_sql::{IntoSqlValue, ModelDescriptor, SqlValue, UpdateModelInput};
use rusqlite::params_from_iter;

use crate::{FromRusqliteRow, RusqliteError, RusqliteRuntime, SqlValueParam};

use super::support::{
    err_item, finalize, item_error, ok_item, reject_duplicate_pks, validate_batch_size,
};

/// `(id, patch)` per item. The embedded layer doesn't enforce policy or
/// `@version`, so there's no `if_match` slot — that's a server-only
/// concern in v1.
pub type BatchUpdateItem<PK, I> = (PK, I);

pub struct BatchUpdate<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a RusqliteRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) items: Vec<BatchUpdateItem<PK, I>>,
}

impl<'a, M: 'static, PK: 'static, I> BatchUpdate<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
    PK: Clone + Eq + Hash + IntoSqlValue,
{
    pub fn run(self) -> Result<BatchResponse<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        validate_batch_size(self.items.len())?;
        let ids: Vec<PK> = self.items.iter().map(|(id, _)| id.clone()).collect();
        reject_duplicate_pks(&ids)?;
        if self.items.is_empty() {
            return Ok(finalize::<M>(vec![]));
        }

        self.runtime.with_connection(|conn| {
            let mut tx = conn.transaction()?;
            let mut results = Vec::with_capacity(self.items.len());
            for (index, (id, input)) in self.items.into_iter().enumerate() {
                let mut sp = tx.savepoint()?;
                match update_one(&mut sp, self.descriptor, id, &input) {
                    Ok(Some(record)) => {
                        sp.commit()?;
                        results.push(ok_item(index, record));
                    }
                    Ok(None) => {
                        sp.rollback()?;
                        results.push(err_item(
                            index,
                            BatchItemError {
                                code: "NOT_FOUND".to_owned(),
                                message: "no row matched".to_owned(),
                            },
                        ));
                    }
                    Err(error) => {
                        sp.rollback()?;
                        results.push(err_item(index, item_error(error)));
                    }
                }
            }
            tx.commit()?;
            Ok(finalize(results))
        })
    }
}

fn update_one<M, PK, I>(
    sp: &mut rusqlite::Savepoint<'_>,
    descriptor: &ModelDescriptor<M, PK>,
    id: PK,
    input: &I,
) -> rusqlite::Result<Option<M>>
where
    I: UpdateModelInput<M>,
    M: FromRusqliteRow,
    PK: IntoSqlValue,
{
    let values = input.sql_values();
    if values.is_empty() {
        // Empty patch isn't a database failure — surface it as NotFound-
        // adjacent at the call site via Ok(None) so the envelope reports
        // a per-item NOT_FOUND. (We could instead introduce a per-item
        // VALIDATION code here, but staying conservative: NOT_FOUND keeps
        // the wire surface small.)
        return Ok(None);
    }

    let mut sql = format!("UPDATE {} SET ", descriptor.table_name);
    let mut binds: Vec<SqlValue> = Vec::with_capacity(values.len() + 1);
    let mut bind_index = 1usize;
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(&format!("{} = ?{}", value.column, bind_index));
        bind_index += 1;
        binds.push(value.value.clone());
    }
    if let Some(version_col) = descriptor.version_column {
        sql.push_str(&format!(", {version_col} = {version_col} + 1"));
    }
    sql.push_str(&format!(" WHERE {} = ?{}", descriptor.primary_key, bind_index));
    binds.push(id.into_sql_value());
    sql.push_str(" RETURNING ");
    sql.push_str(&descriptor.select_projection());

    let mut stmt = sp.prepare(&sql)?;
    let bind_iter = binds.iter().map(SqlValueParam);
    let mut rows = stmt.query(params_from_iter(bind_iter))?;
    match rows.next()? {
        Some(row) => Ok(Some(M::from_rusqlite_row(row)?)),
        None => Ok(None),
    }
}
