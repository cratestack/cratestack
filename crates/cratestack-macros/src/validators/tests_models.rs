//! Which `model`s and procedure `Args` get a `ValidateFields` impl, and what
//! the impl names. The behaviour on the wire is
//! `cratestack-pg/tests/model_argument_validators.rs`.

use super::tests_fixture::{procedure, schema, validating};
use super::{generate_args_validate_impl, generate_model_validate_impl, procedure_validates_args};

#[test]
fn a_model_validates_its_stored_fields_a_client_fills_and_nothing_else() {
    let schema = schema();
    let validating = validating(&schema);
    let names = schema.models.iter().map(|m| m.name.as_str()).collect();
    let impl_of = |name: &str| {
        let model = schema
            .models
            .iter()
            .find(|m| m.name == name)
            .expect("model");
        generate_model_validate_impl(model, &names, &validating).to_string()
    };
    let user = impl_of("User");
    assert!(user.contains("ValidateFields for User"), "{user}");
    // Its own validators, named by their path, the optional one only when set.
    assert!(user.contains("path . field (\"name\")"), "{user}");
    assert!(user.contains("path . field (\"bio\")"), "{user}");
    assert!(user.contains("if let Some (value)"), "{user}");
    // `@server_only` is `#[serde(skip)]`, so it always holds its default and
    // a validator on it would judge a value no client sent; a relation is
    // another row; a `@computed` field is resolved on the way out.
    assert!(!user.contains("secret"), "{user}");
    assert!(!user.contains("posts"), "{user}");
    let shouter = impl_of("Shouter");
    assert!(shouter.contains("\"word\""), "{shouter}");
    assert!(!shouter.contains("shout"), "{shouter}");
    // A model whose only validator is on a `@server_only` field, and one with
    // none, have nothing to validate.
    assert_eq!(impl_of("Vault"), "");
    assert_eq!(impl_of("Bare"), "");
    assert!(!validating.contains("Vault"));
    assert!(!impl_of("User").contains("format"));
}

#[test]
fn args_validate_only_when_an_argument_is_or_holds_a_validated_declaration() {
    let schema = schema();
    let validating = validating(&schema);
    let check = |name: &str| {
        let p = procedure(&schema, name);
        (
            procedure_validates_args(p, &validating),
            generate_args_validate_impl(p, &validating).to_string(),
        )
    };
    let (validates, tokens) = check("open");
    assert!(validates);
    assert!(tokens.contains("ValidateFields for Args"), "{tokens}");
    assert!(tokens.contains("\"args\""), "{tokens}");

    // A `model` as an argument, and one inside a `type` argument.
    let (validates, tokens) = check("takeUser");
    assert!(validates, "a model argument with a validator");
    assert!(tokens.contains("\"args\""), "{tokens}");
    assert!(check("takeWrap").0, "a model nested in a type argument");

    // A model with no validator a client can fill, and plain arguments
    // beside a scalar: no impl and no call.
    assert_eq!(check("takeVault"), (false, String::new()));
    assert_eq!(check("echo"), (false, String::new()));

    // Each validated argument is named by its own argument name; the scalar
    // `id` beside them is not mentioned. A list of models is element-wise.
    let (validates, tokens) = check("many");
    assert!(validates);
    assert!(
        tokens.contains("\"tags\"")
            && tokens.contains("\"extra\"")
            && tokens.contains("\"owners\""),
        "{tokens}"
    );
    assert!(!tokens.contains("\"id\""), "{tokens}");
}
