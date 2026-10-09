//! `BigInt` as a procedure argument and return type, and the arguments'
//! policy value (ADR 0019, PR B, risk 2).

use std::collections::BTreeSet;

use cratestack_core::{Schema, TypeArity};

use super::{procedure_client_output_item_tokens, procedure_type_tokens};
use crate::procedure::generate_procedure_args_struct;
use crate::shared::test_support::{builtin_scalars, render, type_ref, with_rust_decimal};
use crate::validators::Validating;

const BIGINT: &str = ":: cratestack :: BigInt";

const SCHEMA: &str = r#"
type Reply {
  ok Boolean
}

procedure transfer(owner: BigInt, limit: BigInt?, ids: BigInt[]): Reply
  @allow(owner != 7)
"#;

fn schema() -> Schema {
    cratestack_parser::parse_schema(SCHEMA).expect("fixture parses")
}

#[test]
fn bigint_arguments_and_returns_are_the_newtype_in_every_arity() {
    let enums = BTreeSet::new();
    for (arity, expected) in [
        (TypeArity::Required, BIGINT.to_owned()),
        (TypeArity::Optional, format!("Option < {BIGINT} >")),
        (TypeArity::List, format!("Vec < {BIGINT} >")),
    ] {
        assert_eq!(
            render(&procedure_type_tokens(
                &type_ref("BigInt", arity),
                &[],
                &enums
            )),
            expected,
            "{arity:?}"
        );
    }
}

#[test]
fn a_page_of_bigint_names_the_newtype_for_the_client_item() {
    assert_eq!(
        render(&procedure_client_output_item_tokens(&type_ref(
            "BigInt",
            TypeArity::Required
        ))),
        BIGINT
    );
}

/// Scalars the parser refuses in a procedure signature ("not here (e.g.
/// procedure signatures)"), so the table below legitimately lacks them.
const REFUSED_IN_PROCEDURES: [&str; 3] = ["Vector", "Geography", "Geometry"];

#[test]
fn no_builtin_scalar_in_a_procedure_resolves_to_a_user_declared_type() {
    // Guard: both tables end in a catch-all that turns an unknown name into
    // `super::super::<Name>`.
    let enums = BTreeSet::new();
    with_rust_decimal(|| {
        for scalar in builtin_scalars() {
            if REFUSED_IN_PROCEDURES.contains(&scalar) {
                continue;
            }
            let ty = type_ref(scalar, TypeArity::Required);
            for (which, rendered) in [
                (
                    "procedure_type_tokens",
                    render(&procedure_type_tokens(&ty, &[], &enums)),
                ),
                (
                    "procedure_client_output_item_tokens",
                    render(&procedure_client_output_item_tokens(&ty)),
                ),
            ] {
                assert!(
                    !rendered.contains("super"),
                    "{which}({scalar}) fell through to a user-declared type: {rendered}"
                );
            }
        }
    });
}

#[test]
fn the_scalars_without_a_procedure_arm_really_are_refused_by_the_parser() {
    // The exception list above is only sound while the parser keeps them out.
    for scalar in REFUSED_IN_PROCEDURES {
        let arg = if scalar == "Vector" {
            "Vector(3)"
        } else {
            scalar
        };
        let source = format!(
            "type Reply {{\n  ok Boolean\n}}\n\nprocedure p(a: {arg}): Reply\n  @allow(true)\n"
        );
        let error = cratestack_parser::parse_schema(&source)
            .expect_err("the parser must refuse this scalar in a procedure signature");
        assert!(
            error.message().contains("not here"),
            "{scalar}: {}",
            error.message()
        );
    }
}

#[test]
fn the_generated_args_hand_a_policy_a_real_value_for_a_bigint_argument() {
    let schema = schema();
    let enums = BTreeSet::new();
    let args = generate_procedure_args_struct(
        &schema.procedures[0],
        &schema.types,
        &enums,
        "procedure",
        &Validating::none(),
    );
    let rendered = render(&args);
    for expected in [
        format!("pub owner : {BIGINT} ,"),
        format!("pub limit : Option < {BIGINT} > ,"),
        format!("pub ids : Vec < {BIGINT} > ,"),
        "\"owner\" => Some (:: cratestack :: Value :: Int (self . owner . clone () . get ()))"
            .to_owned(),
        "\"ids\" => Some (:: cratestack :: Value :: List".to_owned(),
    ] {
        assert!(
            rendered.contains(&expected),
            "missing `{expected}`: {rendered}"
        );
    }
    // `!=` against a null passes, which is the fail-open this pins: no
    // BigInt argument may be reported to the policy as a bare `Null`.
    assert!(
        !rendered.contains("\"owner\" => Some (:: cratestack :: Value :: Null)"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("\"ids\" => Some (:: cratestack :: Value :: Null)"),
        "{rendered}"
    );
}
