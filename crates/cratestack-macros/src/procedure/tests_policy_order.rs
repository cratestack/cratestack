//! Which error a procedure with two faulty policy attributes reports first
//! (cratestack#1388). Each attribute is lowered and then audited, in
//! declaration order, exactly as on `origin/main` (`ab46b8f5`); running the
//! whole audit up front would instead report the second attribute's error.
//! Built by hand, because the parser refuses `@authorize(bad)` itself.

use super::policies::collect_procedure_policies;
use super::tests_policy_audit::schema;

fn first_error(first: &str, second: &str) -> String {
    let mut schema = schema();
    let procedure = &mut schema.procedures[0];
    procedure.attributes[0].raw = first.to_owned();
    procedure.attributes[1].raw = second.to_owned();
    match collect_procedure_policies(&schema.procedures[0], &schema.models, &schema.types) {
        Ok(_) => panic!("`{first}` + `{second}` must be refused"),
        Err(error) => error,
    }
}

#[test]
fn a_malformed_authorize_is_reported_before_a_later_misspelt_allow() {
    let error = first_error("@authorize(bad)", "@Allow(flag)");
    assert!(error.contains("invalid @authorize attribute"), "{error}");
}

#[test]
fn a_misspelt_allow_is_reported_before_a_later_malformed_authorize() {
    let error = first_error("@Allow(flag)", "@authorize(bad)");
    assert!(error.contains("`@Allow(flag)`"), "{error}");
    assert!(!error.contains("invalid @authorize"), "{error}");
}
