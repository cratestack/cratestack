//! `db.transaction(..)` inside an `@isolation` procedure
//! (docs/design/procedure-isolation.md §7).

use cratestack_core::CratestackError;

use crate::error::cratestack_error_from_sqlx;
use crate::sqlx;
use crate::transaction::Tx;

const NESTED_SAVEPOINT: &str = "cratestack_isolated_nested";

/// `db.transaction(..)` inside an `@isolation` procedure: bracket `body`
/// with a named savepoint on the isolated transaction. Only one level can
/// exist — the handle's lock is held for the whole of `body` — so a fixed
/// name is enough.
///
/// `tx` is raw: `body` can leave the transaction aborted (a statement that
/// failed and was not propagated) or end it (`COMMIT`, `ROLLBACK`). Either
/// way the savepoint cannot be released or rolled back to, and the attempt
/// is poisoned — never committed, whatever the procedure returns — rather
/// than handed to a `COMMIT` that Postgres would turn into a silent
/// `ROLLBACK` behind a success response (GHSA-r67q-4qqq-g9gm).
pub(crate) async fn nested_in_bound<F, T>(
    bound: &super::BoundTx,
    body: F,
) -> Result<T, CratestackError>
where
    F: AsyncFnOnce(&mut Tx) -> Result<T, CratestackError>,
{
    let mut guard = bound.lock()?;
    let tx = guard.tx()?;
    let mut open = OpenSavepoint {
        bound,
        finished: false,
    };
    let result = async {
        savepoint_statement(tx, "SAVEPOINT").await?;
        match body(&mut *tx).await {
            Ok(value) => match savepoint_statement(tx, "RELEASE SAVEPOINT").await {
                Ok(()) => Ok(value),
                Err(error) => {
                    poison(bound, &error);
                    Err(error)
                }
            },
            Err(error) => {
                if let Err(rollback_error) = savepoint_statement(tx, "ROLLBACK TO SAVEPOINT").await
                {
                    poison(bound, &rollback_error);
                }
                Err(error)
            }
        }
    }
    .await;
    open.finished = true;
    bound.observe(&result);
    result
}

/// Poisons the attempt if `transaction(..)` is dropped before its savepoint
/// was released or rolled back to — the caller cancelled it (a timeout,
/// `select!`) or it panicked. The savepoint is then still open with part of
/// the closure's writes, and a `COMMIT` would keep them.
struct OpenSavepoint<'a> {
    bound: &'a super::BoundTx,
    finished: bool,
}

impl Drop for OpenSavepoint<'_> {
    fn drop(&mut self) {
        if !self.finished {
            poison(
                self.bound,
                &CratestackError::Internal(
                    "db.transaction(..) was dropped before it finished".to_owned(),
                ),
            );
        }
    }
}

async fn savepoint_statement(tx: &mut Tx, verb: &str) -> Result<(), CratestackError> {
    sqlx::query(sqlx::AssertSqlSafe(format!("{verb} {NESTED_SAVEPOINT}")))
        .execute(&mut ***tx)
        .await
        .map(|_| ())
        .map_err(cratestack_error_from_sqlx)
}

fn poison(bound: &super::BoundTx, cause: &CratestackError) {
    tracing::warn!(
        target: "cratestack",
        cratestack_error = cause.code(),
        "db.transaction(..) inside an @isolation procedure left the transaction aborted or \
         ended; the attempt will not be committed",
    );
    bound.poison(CratestackError::Internal(format!(
        "db.transaction(..) inside an @isolation procedure could not close its savepoint ({}): \
         a statement in it failed without the error being returned, or it ended the \
         transaction; the procedure's work was rolled back",
        cause.code(),
    )));
}
