//! `BatchUpsert` — per-item SAVEPOINT INSERT ON CONFLICT DO UPDATE inside one
//! outer transaction.

use cratestack_core::BatchResponse;
use cratestack_sql::{ModelDescriptor, SqlValue, UpsertModelInput};
use rusqlite::params_from_iter;

use crate::{
    FromRusqliteRow, RusqliteError, RusqliteRuntime, SqlValueParam, render::render_upsert,
};

use super::support::{
    err_item, finalize, item_error, ok_item, reject_duplicate_sql_values, validate_batch_size,
};

pub struct BatchUpsert<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a RusqliteRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) inputs: Vec<I>,
}

impl<'a, M: 'static, PK: 'static, I> BatchUpsert<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    pub fn run(self) -> Result<BatchResponse<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        validate_batch_size(self.inputs.len())?;
        let pks: Vec<SqlValue> = self
            .inputs
            .iter()
            .map(UpsertModelInput::primary_key_value)
            .collect();
        reject_duplicate_sql_values(&pks)?;
        if self.inputs.is_empty() {
            return Ok(finalize::<M>(vec![]));
        }

        self.runtime.with_connection(|conn| {
            let mut tx = conn.transaction()?;
            let mut results = Vec::with_capacity(self.inputs.len());
            for (index, input) in self.inputs.into_iter().enumerate() {
                let mut sp = tx.savepoint()?;
                match upsert_one(&mut sp, self.descriptor, &input) {
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

fn upsert_one<M, PK, I>(
    sp: &mut rusqlite::Savepoint<'_>,
    descriptor: &ModelDescriptor<M, PK>,
    input: &I,
) -> rusqlite::Result<M>
where
    I: UpsertModelInput<M>,
    M: FromRusqliteRow,
{
    let dialect = cratestack_sql::SqliteDialect;
    let values = input.sql_values();
    let (sql, binds) = render_upsert(&dialect, descriptor, &values);
    let mut stmt = sp.prepare(&sql)?;
    let bind_iter = binds.iter().map(SqlValueParam);
    let mut rows = stmt.query(params_from_iter(bind_iter))?;
    let row = rows
        .next()?
        .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
    M::from_rusqlite_row(row)
}
