//! [`PolicyDb`]: where a multi-statement policy evaluation runs.
//!
//! Most write paths run one main statement on a generic `sqlx::Executor`
//! and are fine with that. Create-policy evaluation is different: it may
//! issue one `EXISTS` probe per relation predicate, recursively, so it
//! needs an executor it can use more than once. A shared `&PgPool` can be
//! copied; a `&mut PgConnection` can only be reborrowed. This enum lets
//! one evaluator serve both.
//!
//! Which one a write uses is [`PolicyDb::of`]'s decision
//! (docs/design/procedure-isolation.md §4.1): inside an `@isolation`
//! procedure the policy reads run on the procedure's own transaction, so
//! the decision is made on the same snapshot as the write and never needs
//! a second pooled connection; everywhere else they run on the pool, as
//! they always have.

use crate::{SqlxRuntime, sqlx};

pub(crate) enum PolicyDb<'a> {
    Pool(&'a sqlx::PgPool),
    Conn(&'a mut sqlx::PgConnection),
}

impl<'a> PolicyDb<'a> {
    /// Where a write running on `conn` reads its policies: `conn` itself
    /// when `runtime` is bound to an `@isolation` attempt (`conn` is then
    /// that attempt's transaction, or a savepoint of it), otherwise the
    /// pool — a second connection, outside any caller's transaction, which
    /// is what every caller without `@isolation` has always had.
    pub(crate) fn of(runtime: &'a SqlxRuntime, conn: &'a mut sqlx::PgConnection) -> Self {
        match runtime.bound() {
            Some(_) => PolicyDb::Conn(conn),
            None => PolicyDb::Pool(runtime.pool()),
        }
    }

    /// A shorter-lived handle onto the same executor, for one statement.
    pub(crate) fn reborrow(&mut self) -> PolicyDb<'_> {
        match self {
            PolicyDb::Pool(pool) => PolicyDb::Pool(pool),
            PolicyDb::Conn(conn) => PolicyDb::Conn(conn),
        }
    }
}
