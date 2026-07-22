#![cfg(test)]

use super::Severity;
use super::test_support::{categories, diff};

const BASE_MODEL: &str = "model Account {\n  id Int @id\n}\n";

#[test]
fn procedure_removal_is_breaking() {
    let prev = format!("{BASE_MODEL}\nprocedure ping(nonce: String): String\n");
    let next = BASE_MODEL.to_owned();

    let result = diff(&prev, &next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["procedure_removed"]
    );
}

#[test]
fn new_procedure_is_additive() {
    let prev = BASE_MODEL.to_owned();
    let next = format!("{BASE_MODEL}\nprocedure ping(nonce: String): String\n");

    let result = diff(&prev, &next);
    assert!(!result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Additive),
        vec!["procedure_added"]
    );
}

#[test]
fn procedure_return_type_change_is_breaking() {
    let prev = format!("{BASE_MODEL}\nprocedure getCount(): Int\n");
    let next = format!("{BASE_MODEL}\nprocedure getCount(): String\n");

    let result = diff(&prev, &next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["procedure_return_type_changed"]
    );
}

#[test]
fn procedure_kind_change_is_breaking() {
    let prev = format!("{BASE_MODEL}\nprocedure sync(id: Int): String\n");
    let next = format!("{BASE_MODEL}\nmutation procedure sync(id: Int): String\n");

    let result = diff(&prev, &next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["procedure_kind_changed"]
    );
}

#[test]
fn procedure_arg_removed_is_breaking() {
    let prev = format!("{BASE_MODEL}\nprocedure getUser(id: Int): String\n");
    let next = format!("{BASE_MODEL}\nprocedure getUser(): String\n");

    let result = diff(&prev, &next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["procedure_arg_removed"]
    );
}

#[test]
fn procedure_arg_added_required_is_breaking() {
    let prev = format!("{BASE_MODEL}\nprocedure getUser(id: Int): String\n");
    let next =
        format!("{BASE_MODEL}\nprocedure getUser(id: Int, includeArchived: Boolean): String\n");

    let result = diff(&prev, &next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["procedure_arg_added_required"]
    );
}

#[test]
fn procedure_arg_added_optional_is_additive() {
    let prev = format!("{BASE_MODEL}\nprocedure getUser(id: Int): String\n");
    let next =
        format!("{BASE_MODEL}\nprocedure getUser(id: Int, includeArchived: Boolean?): String\n");

    let result = diff(&prev, &next);
    assert!(!result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Additive),
        vec!["procedure_arg_added"]
    );
}

#[test]
fn procedure_arg_retype_is_breaking() {
    let prev = format!("{BASE_MODEL}\nprocedure getUser(id: Int): String\n");
    let next = format!("{BASE_MODEL}\nprocedure getUser(id: String): String\n");

    let result = diff(&prev, &next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["procedure_arg_retyped"]
    );
}
