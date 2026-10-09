//! The create path is three-valued, combinators: Unknown has to survive `and`
//! and `or` by Kleene's tables and meet the allow and deny lists. See
//! `create_deny` for the rule and the harness.

use cratestack_core::Value;

use super::create_deny::{allow_grants, decide, deny_fires, leaf};
use super::create_policies::{EQ_AUTH, amount, claim};
use crate::{PolicyExpr, ReadPolicy, ReadPredicate, SqlValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum T3 {
    True,
    False,
    Unknown,
}

/// A leaf with a known truth under an authenticated context whose `tenant`
/// claim is a string, over a `BigInt` column.
fn leaf_of(truth: T3) -> PolicyExpr {
    match truth {
        T3::True => leaf(ReadPredicate::AuthNotNull),
        T3::False => leaf(ReadPredicate::AuthIsNull),
        T3::Unknown => leaf(EQ_AUTH),
    }
}

fn group(and: bool, children: &[PolicyExpr]) -> PolicyExpr {
    let children: &'static [PolicyExpr] = Box::leak(children.to_vec().into_boxed_slice());
    if and {
        PolicyExpr::And(children)
    } else {
        PolicyExpr::Or(children)
    }
}

/// Kleene's tables, written out by hand (not derived from the code under test).
fn kleene(and: bool, left: T3, right: T3) -> T3 {
    use T3::{False, True, Unknown};
    match (and, left, right) {
        (true, False, _) | (true, _, False) => False,
        (true, True, True) => True,
        (true, _, _) => Unknown,
        (false, True, _) | (false, _, True) => True,
        (false, False, False) => False,
        (false, _, _) => Unknown,
    }
}

/// Unknown has to survive `and` / `or` and meet the list rule at the top: a
/// `@deny` fires unless the whole expression is False, an `@allow` grants only
/// if it is True.
#[tokio::test]
async fn unknown_propagates_through_and_or_by_kleene_logic() {
    let ctx = claim(Value::String("7".to_owned()));
    let values = amount(SqlValue::BigInt(7));
    let all = [T3::True, T3::False, T3::Unknown];
    for and in [true, false] {
        for left in all {
            for right in all {
                let expr = group(and, &[leaf_of(left), leaf_of(right)]);
                let expected = kleene(and, left, right);
                let name = if and { "and" } else { "or" };
                assert_eq!(
                    deny_fires(expr, &values, &ctx).await,
                    expected != T3::False,
                    "@deny {left:?} {name} {right:?} is {expected:?}"
                );
                assert_eq!(
                    allow_grants(expr, &values, &ctx).await,
                    expected == T3::True,
                    "@allow {left:?} {name} {right:?} is {expected:?}"
                );
            }
        }
    }
}

/// Three operands and nesting: the result is decided by any False in an `and`
/// and any True in an `or`, wherever it stands, however deep.
#[tokio::test]
async fn nested_groups_are_decided_by_a_false_or_a_true_wherever_it_stands() {
    use T3::{False, True, Unknown};
    let ctx = claim(Value::String("7".to_owned()));
    let values = amount(SqlValue::BigInt(7));
    let u = leaf_of(Unknown);
    let t = leaf_of(True);
    let f = leaf_of(False);
    let cases: [(PolicyExpr, T3); 7] = [
        (group(true, &[u, u, f]), False),
        (group(true, &[f, u, u]), False),
        (group(true, &[u, t, u]), Unknown),
        (group(false, &[u, u, t]), True),
        (group(false, &[u, f, u]), Unknown),
        // And[U, Or[F, U]] = And[U, U].
        (group(true, &[u, group(false, &[f, u])]), Unknown),
        // Or[F, And[U, F]] = Or[F, F].
        (group(false, &[f, group(true, &[u, f])]), False),
    ];
    for (expr, expected) in cases {
        assert_eq!(
            deny_fires(expr, &values, &ctx).await,
            expected != False,
            "{expr:?} is {expected:?}"
        );
        assert_eq!(
            allow_grants(expr, &values, &ctx).await,
            expected == True,
            "{expr:?} is {expected:?}"
        );
    }
}

/// Across policies: any `@deny` that is not False refuses, whichever
/// position it holds; an Unknown `@allow` does not stop a later one granting.
#[tokio::test]
async fn the_lists_combine_per_policy() {
    let ctx = claim(Value::String("7".to_owned()));
    let values = amount(SqlValue::BigInt(7));
    let yes = ReadPolicy {
        expr: leaf_of(T3::True),
    };
    let no = ReadPolicy {
        expr: leaf_of(T3::False),
    };
    let unknown = ReadPolicy {
        expr: leaf_of(T3::Unknown),
    };

    assert!(decide(&[yes], &[no], &values, &ctx).await);
    assert!(!decide(&[yes], &[no, unknown], &values, &ctx).await);
    assert!(!decide(&[yes], &[unknown, no], &values, &ctx).await);
    assert!(!decide(&[yes], &[yes, no], &values, &ctx).await);

    assert!(decide(&[unknown, yes], &[], &values, &ctx).await);
    assert!(decide(&[no, unknown, yes], &[], &values, &ctx).await);
    assert!(!decide(&[unknown, no], &[], &values, &ctx).await);
    assert!(!decide(&[], &[], &values, &ctx).await, "no allow, no grant");
}
