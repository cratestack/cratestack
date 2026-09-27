//! One operation on the bound transaction: a savepoint around it, so a
//! failed statement does not abort the whole `@isolation` transaction, and
//! the two macros every executing path uses to get there.

use cratestack_core::CratestackError;
use sqlx_core::acquire::Acquire as _;

use crate::error::cratestack_error_from_sqlx;
use crate::sqlx;
use crate::transaction::Tx;

/// Open a savepoint on the bound transaction (sqlx issues `SAVEPOINT`
/// because the connection is already inside a transaction).
pub(crate) async fn begin_savepoint(
    tx: &mut Tx,
) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, CratestackError> {
    let outer: &mut sqlx::Transaction<'static, sqlx::Postgres> = tx;
    outer.begin().await.map_err(cratestack_error_from_sqlx)
}

/// Release the savepoint on `Ok`, roll back to it on `Err`, so one
/// failed operation does not abort the whole isolated transaction.
pub(crate) async fn finish_savepoint<T>(
    savepoint: sqlx::Transaction<'_, sqlx::Postgres>,
    result: Result<T, CratestackError>,
) -> Result<T, CratestackError> {
    match result {
        Ok(value) => {
            savepoint
                .commit()
                .await
                .map_err(cratestack_error_from_sqlx)?;
            Ok(value)
        }
        Err(error) => {
            let _ = savepoint.rollback().await;
            Err(error)
        }
    }
}

/// Run `$body` — an expression over `$sp: &mut sqlx::Transaction<'_,
/// Postgres>` producing a future — in its own savepoint of `$bound`, and
/// record a retriable failure on it. A macro rather than a generic `async
/// fn` over a closure so the borrow of the savepoint needs no
/// higher-ranked closure bound.
macro_rules! in_bound_savepoint {
    ($bound:expr, |$sp:ident| $body:expr) => {{
        let bound: &$crate::bound::BoundTx = $bound;
        let result = match bound.lock() {
            Err(error) => Err(error),
            Ok(mut guard) => match guard.tx() {
                Err(error) => Err(error),
                Ok(tx) => match $crate::bound::begin_savepoint(tx).await {
                    Err(error) => Err(error),
                    Ok(mut savepoint) => {
                        let result = {
                            let $sp = &mut savepoint;
                            $body.await
                        };
                        $crate::bound::finish_savepoint(savepoint, result).await
                    }
                },
            },
        };
        bound.observe(&result);
        result
    }};
}

/// The transaction a multi-statement write (the batch builders) runs in:
/// on a pool runtime a fresh one, committed when `$body` returns `Ok`
/// (dropped, so rolled back, otherwise — as before); on a bound runtime a
/// savepoint of the `@isolation` transaction. `$body` is an `async` block
/// over `$tx: &mut sqlx::Transaction<'_, Postgres>`.
macro_rules! in_write_tx {
    ($runtime:expr, |$tx:ident| $body:expr) => {{
        let runtime: &$crate::SqlxRuntime = $runtime;
        match runtime.bound() {
            Some(bound) => $crate::bound::in_bound_savepoint!(bound, |$tx| $body),
            None => {
                let mut owned = runtime
                    .pool()
                    .begin()
                    .await
                    .map_err($crate::cratestack_error_from_sqlx)?;
                let result = {
                    let $tx = &mut owned;
                    $body.await
                };
                match result {
                    Ok(value) => owned
                        .commit()
                        .await
                        .map(|()| value)
                        .map_err($crate::cratestack_error_from_sqlx),
                    Err(error) => Err(error),
                }
            }
        }
    }};
}

pub(crate) use {in_bound_savepoint, in_write_tx};
