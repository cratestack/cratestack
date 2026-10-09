//! Which `type`s get a `ValidateFields` impl, and what the impl names. The
//! behaviour on the wire is `cratestack-api/tests/type_validators_{rest,rpc}.rs`.

use super::generate_type_validate_impl;
use super::tests_fixture::{schema, validating};

#[test]
fn a_declaration_validates_when_it_holds_a_validator_directly_or_through_another() {
    let schema = schema();
    assert_eq!(
        validating(&schema).names(),
        [
            "Account", "Inner", "Left", "Middle", "Node", "Outer", "Owner", "Post", "Right",
            "Shouter", "Tag", "User", "Wrap"
        ]
    );
}

/// `Chain { next Chain? }` cannot compile (E0072, infinite size), so it
/// proved nothing; a list cycle compiles. The fixpoint finishes on every
/// shape of cycle and calls none of them validating without a validator.
#[test]
fn a_cycle_through_a_list_terminates_and_validation_recurses() {
    let schema = schema();
    let validating = validating(&schema);
    let names = validating.names();
    // Self cycle with a validator; mutual cycle with a validator on one side.
    assert!(names.contains(&"Node"), "{names:?}");
    assert!(
        names.contains(&"Left") && names.contains(&"Right"),
        "{names:?}"
    );
    // No validator anywhere in the cycle.
    assert!(!names.contains(&"Loop"), "{names:?}");

    let impl_of = |name: &str| {
        let ty = schema.types.iter().find(|t| t.name == name).expect("type");
        generate_type_validate_impl(ty, &validating).to_string()
    };
    // `Node` validates its own label, then every child, which is a `Node`
    // again: the generated body calls `validate_at` on each element.
    let node = impl_of("Node");
    assert!(node.contains("validate_length"), "{node}");
    assert!(
        node.contains("self . children . iter () . enumerate ()"),
        "{node}"
    );
    assert!(node.contains("item . validate_at"), "{node}");
    // `Left` has no validator of its own and reaches one only through `Right`.
    assert!(impl_of("Left").contains("item . validate_at"));
    assert_eq!(impl_of("Loop"), "");
}

#[test]
fn only_a_validating_type_gets_an_impl() {
    let schema = schema();
    let validating = validating(&schema);
    let impl_of = |name: &str| {
        let ty = schema.types.iter().find(|t| t.name == name).expect("type");
        generate_type_validate_impl(ty, &validating).to_string()
    };
    assert!(impl_of("Tag").contains("validate_length"));
    // A list recurses element by element under `name[index]`; an optional
    // only when present; a required field under its own name.
    let owner = impl_of("Owner");
    assert!(owner.contains("enumerate"), "{owner}");
    assert!(owner.contains("list_path . index (index)"), "{owner}");
    assert!(owner.contains("if let Some (inner)"), "{owner}");
    assert!(impl_of("Account").contains("path . field (\"owner\")"));
    // A `model` held by a `type` is validated in place, like a `type`.
    assert!(impl_of("Wrap").contains("self . owner . validate_at"));
    assert_eq!(impl_of("Plain"), "");
    assert_eq!(impl_of("Reply"), "");
}

/// A valid request must not build a path string: a path is a `FieldPath`
/// on the stack and is written only by the validator that fails.
#[test]
fn no_impl_formats_a_path_on_the_success_path() {
    let schema = schema();
    let validating = validating(&schema);
    for ty in &schema.types {
        let tokens = generate_type_validate_impl(ty, &validating).to_string();
        assert!(!tokens.contains("format"), "{}: {tokens}", ty.name);
        assert!(!tokens.contains("to_string"), "{}: {tokens}", ty.name);
    }
}
