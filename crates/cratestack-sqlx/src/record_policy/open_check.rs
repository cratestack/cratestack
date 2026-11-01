//! The open-time half of a subscription's read policy
//! (`docs/design/subscriptions-websocket.md` §5.1): before a feed opens,
//! decide whether this caller could read *any* row at all. If no row could
//! pass, the subscription is refused (`unauthenticated` for an anonymous
//! caller, else `permission_denied`) instead of opening a feed that will
//! never deliver anything.
//!
//! Each predicate is reduced to the set of truth values it can take over all
//! rows: a predicate on the caller alone (`auth() != null`, `hasRole(..)`) has
//! exactly one, decided now; a predicate on a column has all three, unless it
//! compares the column with an `auth()` claim the caller lacks, which the SQL
//! path renders as `FALSE` for every row. The sets combine pairwise under
//! Kleene's `and`/`or`. Treating predicates as independent over-approximates
//! what is possible, so this can open a feed that will deliver nothing; it
//! never refuses one that could deliver something.

use cratestack_core::{CratestackContext, CratestackError};

use crate::query::{Truth, auth_value_to_sql, evaluate_input_truth};
use crate::{PolicyExpr, ReadPolicy, ReadPredicate};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Possible(u8);

impl Possible {
    const TRUE: u8 = 1;
    const FALSE: u8 = 2;
    const UNKNOWN: u8 = 4;
    const ANY: Self = Self(Self::TRUE | Self::FALSE | Self::UNKNOWN);

    fn of(truth: Truth) -> Self {
        Self(match truth {
            Truth::True => Self::TRUE,
            Truth::False => Self::FALSE,
            Truth::Unknown => Self::UNKNOWN,
        })
    }

    fn values(self) -> impl Iterator<Item = Truth> {
        [
            (Self::TRUE, Truth::True),
            (Self::FALSE, Truth::False),
            (Self::UNKNOWN, Truth::Unknown),
        ]
        .into_iter()
        .filter(move |(bit, _)| self.0 & bit != 0)
        .map(|(_, truth)| truth)
    }

    fn combine(self, other: Self, op: fn(Truth, Truth) -> Truth) -> Self {
        let mut out = 0;
        for a in self.values() {
            for b in other.values() {
                out |= Self::of(op(a, b)).0;
            }
        }
        Self(out)
    }

    fn can_be(self, truth: Truth) -> bool {
        self.0 & Self::of(truth).0 != 0
    }
}

fn possible(expr: PolicyExpr, ctx: &CratestackContext) -> Possible {
    match expr {
        PolicyExpr::Predicate(predicate) => predicate_possible(predicate, ctx),
        PolicyExpr::And(exprs) => exprs.iter().fold(Possible::of(Truth::True), |acc, expr| {
            acc.combine(possible(*expr, ctx), Truth::and)
        }),
        PolicyExpr::Or(exprs) => exprs.iter().fold(Possible::of(Truth::False), |acc, expr| {
            acc.combine(possible(*expr, ctx), Truth::or)
        }),
    }
}

fn predicate_possible(predicate: ReadPredicate, ctx: &CratestackContext) -> Possible {
    match predicate {
        ReadPredicate::FieldEqAuth { auth_field, .. }
        | ReadPredicate::FieldNeAuth { auth_field, .. } => {
            if auth_value_to_sql(ctx, auth_field).is_some() {
                Possible::ANY
            } else {
                Possible::of(Truth::False)
            }
        }
        // `EXISTS` is never NULL.
        ReadPredicate::Relation { .. } => Possible(Possible::TRUE | Possible::FALSE),
        ReadPredicate::FieldIsTrue { .. }
        | ReadPredicate::FieldEqLiteral { .. }
        | ReadPredicate::FieldNeLiteral { .. }
        | ReadPredicate::FieldInLiterals { .. }
        | ReadPredicate::FieldNotInLiterals { .. } => Possible::ANY,
        ReadPredicate::AuthNotNull
        | ReadPredicate::AuthIsNull
        | ReadPredicate::AuthIsSystem
        | ReadPredicate::HasRole { .. }
        | ReadPredicate::InTenant { .. }
        | ReadPredicate::AuthFieldEqLiteral { .. }
        | ReadPredicate::AuthFieldNeLiteral { .. } => {
            Possible::of(evaluate_input_truth(predicate, &[], ctx))
        }
    }
}

/// Whether some row could pass `allow`/`deny` for `ctx`: an `@allow` can be
/// `True`, and no `@deny` fires on every row (it fires on anything but
/// `False`). An empty allow list denies, as on every read.
pub fn may_read_some_row(
    allow: &[ReadPolicy],
    deny: &[ReadPolicy],
    ctx: &CratestackContext,
) -> bool {
    let allowed = allow
        .iter()
        .any(|policy| possible(policy.expr, ctx).can_be(Truth::True));
    let always_denied = deny
        .iter()
        .any(|policy| !possible(policy.expr, ctx).can_be(Truth::False));
    allowed && !always_denied
}

/// Refuses a subscription to `model` that could never deliver a row.
pub fn refuse_unreadable_subscription(
    model: &str,
    allow: &[ReadPolicy],
    deny: &[ReadPolicy],
    ctx: &CratestackContext,
) -> Result<(), CratestackError> {
    if may_read_some_row(allow, deny, ctx) {
        Ok(())
    } else if !ctx.is_authenticated() {
        Err(CratestackError::Unauthorized(format!(
            "subscribing to `{model}` requires authentication"
        )))
    } else {
        Err(CratestackError::Forbidden(format!(
            "no `{model}` record is readable by this caller"
        )))
    }
}

#[cfg(test)]
mod tests;
