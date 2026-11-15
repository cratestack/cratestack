use super::*;

fn op(allow: &[&str], deny: &[&str]) -> OpPolicyAnalysis {
    analyse_op_policy(allow, deny)
}

#[test]
fn a_disjunction_of_roles_names_both_and_needs_a_principal() {
    let verdict = op(&[r#"hasRole("a") || hasRole("b")"#], &[]);
    assert_eq!(verdict.roles, ["a", "b"]);
    assert!(verdict.requires_principal);
    assert!(verdict.unanalysed.is_empty());
}

#[test]
fn a_conjunction_needs_a_principal_when_any_branch_does() {
    assert!(op(&[r#"hasRole("a") && x == 1"#], &[]).requires_principal);
    assert!(!op(&["x == 1 && y == 2"], &[]).requires_principal);
}

#[test]
fn a_disjunction_needs_a_principal_only_when_every_branch_does() {
    assert!(!op(&[r#"hasRole("a") || x == 1"#], &[]).requires_principal);
    assert!(op(&[r#"hasRole("a") || auth() != null"#], &[]).requires_principal);
}

#[test]
fn literals_and_anonymous_checks_do_not_need_a_principal() {
    assert!(!op(&["true"], &[]).requires_principal);
    assert!(!op(&["auth() == null"], &[]).requires_principal);
    assert!(op(&["false"], &[]).requires_principal);
}

#[test]
fn principal_checks_need_one() {
    for term in [
        "auth() != null",
        "auth().isSystem()",
        "isSystem()",
        r#"inTenant("t")"#,
        "auth().id == owner",
        "owner == auth().id",
        r#"auth().tier != "free""#,
    ] {
        assert!(op(&[term], &[]).requires_principal, "{term}");
    }
}

#[test]
fn input_and_field_comparisons_do_not_need_a_principal() {
    for term in [
        "x == 1",
        "a != b",
        "published",
        r#"status in [Open, Closed]"#,
    ] {
        let verdict = op(&[term], &[]);
        assert!(!verdict.requires_principal, "{term}");
        assert!(verdict.unanalysed.is_empty(), "{term}");
    }
}

#[test]
fn a_deny_adds_no_roles_and_does_not_change_the_verdict() {
    let verdict = op(&["auth() != null"], &[r#"hasRole("banned")"#]);
    assert!(verdict.roles.is_empty());
    assert!(verdict.requires_principal);
}

#[test]
fn several_allows_are_alternatives() {
    let verdict = op(&[r#"hasRole("b")"#, r#"hasRole("a") || hasRole("b")"#], &[]);
    assert_eq!(verdict.roles, ["a", "b"]);
    assert!(verdict.requires_principal);
    assert!(!op(&[r#"hasRole("a")"#, "true"], &[]).requires_principal);
}

#[test]
fn no_allow_is_default_deny_which_no_anonymous_caller_passes() {
    assert!(op(&[], &[]).requires_principal);
}

#[test]
fn what_the_analysis_does_not_understand_is_anonymous_and_listed() {
    let verdict = op(
        &[
            r#"hasRole("a") && frobnicate("x")"#,
            "x ==",
            "a in auth().roles",
        ],
        &[],
    );
    assert!(!verdict.requires_principal);
    assert_eq!(
        verdict.unanalysed,
        [
            r#"hasRole("a") && frobnicate("x")"#,
            "x ==",
            "a in auth().roles"
        ]
    );
    assert_eq!(verdict.roles, ["a"]);
    assert_eq!(op(&["true"], &["???("]).unanalysed, ["???("]);
}
