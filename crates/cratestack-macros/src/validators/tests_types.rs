//! Which `type`s and procedure `Args` get a `ValidateFields` impl, and what
//! the impl names. The behaviour on the wire is
//! `cratestack-api/tests/type_validators_{rest,rpc}.rs`.

use super::{
    generate_args_validate_impl, generate_type_validate_impl, procedure_validates_args,
    validating_type_names,
};

const SCHEMA: &str = r#"
datasource db {
  provider = "none"
}

type Tag {
  label String @length(min: 2)
}

type Owner {
  tags Tag[]
  backup Tag?
}

type Account {
  owner Owner
}

type Plain {
  note String
}

// Refers to itself and holds no validator: the fixpoint must terminate and
// must not call it validating.
type Chain {
  next Chain?
}

type Reply {
  ok Boolean
}

procedure open(args: Account): Reply
  @allow(true)

procedure echo(args: Plain, count: Int): Reply
  @allow(true)

procedure many(id: String, tags: Tag[], extra: Tag?): Reply
  @allow(true)
"#;

fn schema() -> cratestack_core::Schema {
    cratestack_parser::parse_schema(SCHEMA).expect("fixture parses")
}

fn procedure<'a>(
    schema: &'a cratestack_core::Schema,
    name: &str,
) -> &'a cratestack_core::Procedure {
    schema
        .procedures
        .iter()
        .find(|p| p.name == name)
        .expect("procedure")
}

#[test]
fn a_type_validates_when_it_holds_a_validator_directly_or_through_another_type() {
    let schema = schema();
    let validating = validating_type_names(&schema.types);
    assert_eq!(
        validating.iter().map(String::as_str).collect::<Vec<_>>(),
        ["Account", "Owner", "Tag"]
    );
}

#[test]
fn only_a_validating_type_gets_an_impl() {
    let schema = schema();
    let validating = validating_type_names(&schema.types);
    let impl_of = |name: &str| {
        let ty = schema.types.iter().find(|t| t.name == name).expect("type");
        generate_type_validate_impl(ty, &validating).to_string()
    };
    assert!(impl_of("Tag").contains("validate_length"));
    // A list recurses element by element under `name[index].`; an optional
    // only when present.
    let owner = impl_of("Owner");
    assert!(owner.contains("enumerate"), "{owner}");
    assert!(owner.contains("\"{}{}[{}].\""), "{owner}");
    assert!(owner.contains("if let Some (inner)"), "{owner}");
    assert!(impl_of("Account").contains("\"{}{}.\""));
    assert_eq!(impl_of("Plain"), "");
    assert_eq!(impl_of("Chain"), "");
    assert_eq!(impl_of("Reply"), "");
}

#[test]
fn args_validate_only_when_an_argument_is_or_holds_a_validated_type() {
    let schema = schema();
    let check = |name: &str| {
        let p = procedure(&schema, name);
        (
            procedure_validates_args(p, &schema.types),
            generate_args_validate_impl(p, &schema.types).to_string(),
        )
    };
    let (validates, tokens) = check("open");
    assert!(validates);
    assert!(tokens.contains("ValidateFields for Args"), "{tokens}");
    assert!(tokens.contains("\"args\""), "{tokens}");

    // Plain arguments, beside a scalar: no impl and no call.
    assert_eq!(check("echo"), (false, String::new()));

    // Each validated argument is named by its own argument name; the scalar
    // `id` beside them is not mentioned.
    let (validates, tokens) = check("many");
    assert!(validates);
    assert!(
        tokens.contains("\"tags\"") && tokens.contains("\"extra\""),
        "{tokens}"
    );
    assert!(!tokens.contains("\"id\""), "{tokens}");
}
