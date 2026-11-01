//! Read policy for a record the caller never queried: a model event handed
//! to an `@@subscribe` subscriber (`GET /rpc/subscribe/{op_id}`).
//!
//! A read through a delegate has its policy pushed into the `WHERE` clause.
//! An event has no query: its record arrives from the outbox, so the
//! policy is decided in-process against the record's values, by the same
//! evaluator the create path uses (relation predicates still probe the
//! database with `EXISTS`). Everything else follows the read path: an
//! empty allow list denies, a `@deny` fires on anything not `False`.
//!
//! The evaluator reads an absent column as `False`, which would let a
//! `@deny` over it pass. An event payload lacks every `@server_only`
//! column (`#[serde(skip)]`), so [`missing_policy_columns`] is checked when
//! a subscription opens, and a policy it cannot decide is refused there.

mod open_check;

use cratestack_core::{CratestackContext, CratestackError};

pub use open_check::{may_read_some_row, refuse_unreadable_subscription};

use crate::query::{PolicyDb, evaluate_create_policies};
use crate::{PolicyExpr, ReadPolicy, ReadPredicate, SqlColumnValue, sqlx};

/// Whether `ctx` may read the record whose column values are `values`,
/// under `allow` / `deny` (a model's detail-and-read policies).
pub async fn record_readable(
    pool: &sqlx::PgPool,
    allow: &[ReadPolicy],
    deny: &[ReadPolicy],
    values: &[SqlColumnValue],
    ctx: &CratestackContext,
) -> Result<bool, CratestackError> {
    evaluate_create_policies(PolicyDb::Pool(pool), allow, deny, values, ctx).await
}

/// The columns of the policy's own model that `allow` / `deny` read and
/// `present` does not hold. A relation predicate counts its local join
/// column only: its nested expression runs in SQL against the related
/// table, where every column is there.
pub fn missing_policy_columns(
    allow: &[ReadPolicy],
    deny: &[ReadPolicy],
    present: &[&str],
) -> Vec<&'static str> {
    let mut missing = Vec::new();
    for policy in allow.iter().chain(deny) {
        collect_columns(policy.expr, &mut missing);
    }
    missing.retain(|column| !present.contains(column));
    missing.sort_unstable();
    missing.dedup();
    missing
}

fn collect_columns(expr: PolicyExpr, out: &mut Vec<&'static str>) {
    match expr {
        PolicyExpr::Predicate(predicate) => out.extend(predicate_column(predicate)),
        PolicyExpr::And(exprs) | PolicyExpr::Or(exprs) => {
            for expr in exprs {
                collect_columns(*expr, out);
            }
        }
    }
}

fn predicate_column(predicate: ReadPredicate) -> Option<&'static str> {
    match predicate {
        ReadPredicate::FieldIsTrue { column }
        | ReadPredicate::FieldEqLiteral { column, .. }
        | ReadPredicate::FieldNeLiteral { column, .. }
        | ReadPredicate::FieldInLiterals { column, .. }
        | ReadPredicate::FieldNotInLiterals { column, .. }
        | ReadPredicate::FieldEqAuth { column, .. }
        | ReadPredicate::FieldNeAuth { column, .. } => Some(column),
        ReadPredicate::Relation { parent_column, .. } => Some(parent_column),
        ReadPredicate::AuthNotNull
        | ReadPredicate::AuthIsNull
        | ReadPredicate::AuthIsSystem
        | ReadPredicate::HasRole { .. }
        | ReadPredicate::InTenant { .. }
        | ReadPredicate::AuthFieldEqLiteral { .. }
        | ReadPredicate::AuthFieldNeLiteral { .. } => None,
    }
}

#[cfg(test)]
mod tests;
