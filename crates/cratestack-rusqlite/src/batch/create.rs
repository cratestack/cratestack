//! `BatchCreate` — per-item SAVEPOINT INSERT inside one outer transaction.

use cratestack_core::BatchResponse;
use cratestack_sql::{CreateModelInput, ModelDescriptor, SqlValue};
use rusqlite::params_from_iter;

use crate::{FromRusqliteRow, RusqliteError, RusqliteRuntime, SqlValueParam};

use super::support::{err_item, finalize, item_error, ok_item, validate_batch_size};

pub struct BatchCreate<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a RusqliteRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) inputs: Vec<I>,
}

impl<'a, M: 'static, PK: 'static, I> BatchCreate<'a, M, PK, I>
where
    I: CreateModelInput<M>,
{
    pub fn run(self) -> Result<BatchResponse<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        validate_batch_size(self.inputs.len())?;
        if self.inputs.is_empty() {
            return Ok(finalize::<M>(vec![]));
        }

        self.runtime.with_connection(|conn| {
            let mut tx = conn.transaction()?;
            let mut results = Vec::with_capacity(self.inputs.len());
            for (index, input) in self.inputs.into_iter().enumerate() {
                let mut sp = tx.savepoint()?;
                match insert_one(&mut sp, self.descriptor, &input) {
                    Ok(record) => {
                        sp.commit()?;
                        results.push(ok_item(index, record));
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

fn insert_one<M, PK, I>(
    sp: &mut rusqlite::Savepoint<'_>,
    descriptor: &ModelDescriptor<M, PK>,
    input: &I,
) -> rusqlite::Result<M>
where
    I: CreateModelInput<M>,
    M: FromRusqliteRow,
{
    let values = input.sql_values();
    let mut sql = format!("INSERT INTO {} (", descriptor.table_name);
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(value.column);
    }
    sql.push_str(") VALUES (");
    for idx in 0..values.len() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(&format!("?{}", idx + 1));
    }
    sql.push_str(") RETURNING ");
    sql.push_str(&descriptor.select_projection());

    let binds: Vec<SqlValue> = values.iter().map(|v| v.value.clone()).collect();
    let mut stmt = sp.prepare(&sql)?;
    let bind_iter = binds.iter().map(SqlValueParam);
    let mut rows = stmt.query(params_from_iter(bind_iter))?;
    let row = rows
        .next()?
        .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
    M::from_rusqlite_row(row)
}
