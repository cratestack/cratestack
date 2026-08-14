#![cfg(test)]

//! Regression coverage for a confirmed SQL operator-precedence
//! authorization bypass in [`push_action_policy_query`], found while
//! diagnosing `crates/cratestack-pg/tests/policy_db_auth_engine.rs`
//! (2026-08 audit; see that test's `#[ignore]` reason for the live,
//! Postgres-backed reproduction). This file pins the defect at the
//! SQL-string level so it's caught by a plain `cargo test -p
//! cratestack-sqlx` run — no database required — rather than only by
//! the much slower/heavier PG-backed integration test.
//!
//! `push_action_policy_query` renders a model's allow policies as
//! `A OR B OR ...` (one [`ReadPolicy`] per separate `@@allow("<action>",
//! ...)` schema attribute). When the action has a matching `@@deny`
//! clause, the whole allow disjunction is correctly wrapped:
//! `NOT (deny...) AND (allow...)`. When it does NOT (the common case —
//! most actions have no `@@deny`), the function falls through to
//! emitting the *bare*, unparenthesized `A OR B OR ...` directly. Every
//! call site then does `<row filter> AND ` immediately before calling
//! this function (see `authorize_record_action` / `push_scoped_conditions`
//! in `query/support/conditions.rs`, and every `query/write/*_exec.rs`
//! / `query/batch/*.rs` mutation path). Because SQL's `AND` binds
//! tighter than `OR`, `id = $1 AND A OR B` parses as
//! `(id = $1 AND A) OR B` — the row filter only scopes the *first*
//! allow clause; every other clause becomes an unscoped, table-wide OR
//! that can make the whole predicate true for a row that has nothing
//! to do with `id = $1`.
//!
//! FIXED (2026-08 audit): `push_action_policy_query` now wraps its
//! entire emitted predicate in parentheses on both branches, so the
//! function's contract is "emits one self-contained boolean group" and
//! no call site has to know about the precedence hazard. This test
//! guards that contract and must stay green.

use cratestack_core::{CratestackContext, Value};

use crate::query::push_action_policy_query;
use crate::{PolicyExpr, ReadPolicy, ReadPredicate, sqlx};

/// Two separate `@@allow` clauses (no `@@deny`) — the same shape as
/// `EnginePost` in `crates/cratestack-pg/tests/fixtures/auth_engine.cstack`,
/// which is what the live repro in `policy_db_auth_engine.rs` uses.
fn two_allow_clauses_no_deny() -> [ReadPolicy; 2] {
    [
        ReadPolicy {
            expr: PolicyExpr::Predicate(ReadPredicate::FieldEqAuth {
                column: "author_id",
                auth_field: "id",
            }),
        },
        ReadPolicy {
            expr: PolicyExpr::Predicate(ReadPredicate::FieldIsTrue {
                column: "published",
            }),
        },
    ]
}

#[test]
fn allow_disjunction_is_parenthesized_so_row_filter_cannot_be_bypassed() {
    let ctx =
        CratestackContext::authenticated([("id".to_owned(), Value::String("usr_4".to_owned()))]);
    let allow = two_allow_clauses_no_deny();

    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT 1 FROM engine_posts WHERE ");
    query.push("id = ");
    query.push_bind("post_1".to_owned());
    query.push(" AND ");
    push_action_policy_query(&mut query, &allow, &[], &ctx);

    let sql = query.sql();
    // The allow disjunction must be wrapped as its own group so it
    // can never absorb the preceding `id = $1` filter via `OR`
    // short-circuiting past it.
    assert!(
        sql.contains("id = $1 AND (") && sql.trim_end().ends_with(')'),
        "expected the allow-policy OR-disjunction to be wrapped in its own \
         parentheses so `id = $1 AND (...)` fully scopes it; got: {sql}"
    );
}
