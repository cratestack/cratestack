//! Relation quantifiers over an `auth()` comparison (cratestack#1388, review
//! B1). For an anonymous caller `FieldEqAuth` renders `FALSE`, and what the
//! quantifier makes of that is the verdict: `NOT EXISTS (.. AND FALSE)` is
//! true for every row, and `every` is vacuously true with no related rows,
//! so neither may be reported as needing a principal.

use super::analyse_op_policy;

fn requires(rule: &str) -> bool {
    let verdict = analyse_op_policy(&[rule], &[]);
    assert!(
        verdict.unanalysed.is_empty(),
        "{rule}: {:?}",
        verdict.unanalysed
    );
    verdict.requires_principal
}

#[test]
fn some_and_to_one_hops_keep_the_auth_leaf_false() {
    for rule in [
        "memberships.some.userId == auth().id",
        "auth().id == memberships.some.userId",
        "memberships.some.userId != auth().id",
        "owner.id == auth().id",
        "owner.memberships.some.userId == auth().id",
        "auth() == owner",
    ] {
        assert!(requires(rule), "{rule}");
    }
}

#[test]
fn none_over_an_auth_comparison_holds_for_an_anonymous_caller() {
    for rule in [
        "memberships.none.userId == auth().id",
        "auth().id != memberships.none.userId",
        "memberships.none.userId != auth().id",
    ] {
        assert!(!requires(rule), "{rule}");
    }
}

#[test]
fn every_over_an_auth_comparison_holds_vacuously() {
    for rule in [
        "memberships.every.userId == auth().id",
        "auth().id == memberships.every.userId",
    ] {
        assert!(!requires(rule), "{rule}");
    }
}

#[test]
fn a_quantifier_anywhere_in_the_path_decides_it() {
    for rule in [
        "owner.memberships.none.userId == auth().id",
        "memberships.some.audits.every.userId == auth().id",
        "memberships.none.audits.some.userId == auth().id",
    ] {
        assert!(!requires(rule), "{rule}");
    }
}

#[test]
fn the_quantifier_rule_composes_through_and_and_or() {
    // `&&` is AlwaysFalse if any child is; `||` only if every child is.
    assert!(requires(
        r#"memberships.none.userId == auth().id && hasRole("a")"#
    ));
    assert!(!requires(
        "memberships.none.userId == auth().id && published"
    ));
    assert!(requires(
        "memberships.some.userId == auth().id || hasRole(\"a\")"
    ));
    assert!(!requires(
        "memberships.none.userId == auth().id || hasRole(\"a\")"
    ));
    assert!(requires(
        "memberships.every.userId == auth().id && auth() != null"
    ));
}

#[test]
fn a_none_over_a_plain_comparison_is_unchanged() {
    assert!(!requires("memberships.none.userId == 5"));
    assert!(!requires("memberships.some.active"));
}
