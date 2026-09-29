//! [`PolicyDb`]: where a multi-statement policy evaluation runs.
//!
//! Most write paths run one main statement on a generic `sqlx::Executor`
//! and are fine with that. Create-policy evaluation is different: it may
//! issue one `EXISTS` probe per relation predicate, recursively, so it
//! needs an executor it can use more than once. A shared `&PgPool` can be
//! copied; a `&mut PgConnection` can only be reborrowed. This enum lets
//! one evaluator serve both.
//!
//! A write that runs on a connection reads its policies on that same
//! connection, for every caller (docs/design/procedure-isolation.md §4.1,
//! cratestack#1117): the decision is made on the caller's transaction, sees
//! its own earlier writes, and never needs a second pooled connection.
//! [`PolicyDb::Pool`] exists only for the public `*_with_executor` helpers,
//! whose caller names the pool explicitly.

use crate::sqlx;

pub(crate) enum PolicyDb<'a> {
    Pool(&'a sqlx::PgPool),
    Conn(&'a mut sqlx::PgConnection),
}

impl<'a> PolicyDb<'a> {
    /// A shorter-lived handle onto the same executor, for one statement.
    pub(crate) fn reborrow(&mut self) -> PolicyDb<'_> {
        match self {
            PolicyDb::Pool(pool) => PolicyDb::Pool(pool),
            PolicyDb::Conn(conn) => PolicyDb::Conn(conn),
        }
    }
}
