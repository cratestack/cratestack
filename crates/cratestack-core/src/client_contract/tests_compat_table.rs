//! The classifier's table: every rule of `compat.rs`, must-refuse cases
//! included. Old is `tests_compat::base()`.

use serde_json::json;

use super::tests::{field, ty};
use super::tests_compat::*;
use super::{classify, op_contract_json};
use crate::schema::TypeArity;

const CREATE: &str = "model.Widget.create";
const PING: &str = "procedure.ping";
const PAINT: &str = "procedure.paint";

#[test]
fn an_unchanged_op_is_compatible() {
    let _ = [CREATE, PING, PAINT].map(|key| compatible(key, |_| {}));
}

#[test]
fn a_model_gaining_an_optional_or_defaulted_field_is_compatible() {
    compatible(CREATE, |s| {
        s.models[0]
            .fields
            .push(optional(field("note", "String", &[])));
    });
    compatible(CREATE, |s| {
        s.models[0]
            .fields
            .push(field("tier", "Int", &["@default(1)"]));
    });
}

#[test]
fn a_model_gaining_a_required_field_is_breaking() {
    breaking(CREATE, "`@default` on the op's own model", |s| {
        s.models[0].fields.push(field("tier", "Int", &[]));
    });
}

#[test]
fn removing_retyping_or_loosening_a_field_is_breaking() {
    breaking(CREATE, "`Widget.name` was removed", |s| {
        s.models[0].fields.pop();
    });
    breaking(CREATE, "`Widget.name` changed from String to Int", |s| {
        s.models[0].fields[1].ty = ty("Int");
    });
    // Widget is both an input and an output: required to optional would let
    // the server send null to a client that reads it as required.
    breaking(
        CREATE,
        "`Widget.name` changed from String to String?",
        |s| {
            s.models[0].fields[1].ty.arity = TypeArity::Optional;
        },
    );
}

#[test]
fn an_input_only_field_may_become_optional_an_output_only_one_may_not() {
    compatible(PAINT, |s| {
        type_mut(s, "Paint").fields[1].ty.arity = TypeArity::Optional;
    });
    breaking(PAINT, "`Receipt.total`", |s| {
        type_mut(s, "Receipt").fields[0].ty.arity = TypeArity::Optional;
    });
}

#[test]
fn an_output_only_type_may_gain_any_field_an_input_one_only_optional_ones() {
    compatible(PAINT, |s| {
        type_mut(s, "Receipt").fields.push(field("tax", "Int", &[]));
    });
    breaking(PAINT, "`Paint.size` is", |s| {
        type_mut(s, "Paint").fields.push(field("size", "Int", &[]));
    });
    compatible(PAINT, |s| {
        type_mut(s, "Paint")
            .fields
            .push(optional(field("size", "Int", &[])));
    });
}

#[test]
fn enum_variants_only_ever_append_to_an_input_only_enum() {
    compatible(PAINT, |s| {
        set_variants(s, "Shade", &["Light", "Dark", "Mid"])
    });
    breaking(PAINT, "enum `Shade`", |s| {
        set_variants(s, "Shade", &["Light", "Mid", "Dark"]);
    });
    breaking(PAINT, "enum `Shade`", |s| {
        set_variants(s, "Shade", &["Light"])
    });
    breaking(PAINT, "enum `Shade`", |s| {
        set_variants(s, "Shade", &["Dark", "Light"]);
    });
    // The output enum: an old client's strict decoder would fail on Hot.
    breaking(PAINT, "an old client decodes", |s| {
        set_variants(s, "Tone", &["Warm", "Cool", "Hot"]);
    });
}

#[test]
fn a_return_type_change_is_breaking() {
    breaking(PING, "the return type changed", |s| {
        s.procedures[0].return_type = ty("Paint");
    });
    breaking(PING, "the return type changed", |s| {
        s.procedures[0].return_type.arity = TypeArity::List;
    });
}

#[test]
fn procedure_arguments_follow_the_input_rules() {
    let add = |arity| {
        move |s: &mut crate::schema::Schema| {
            let mut extra = procedure("x", &[("limit", "Int")], "Int").args.remove(0);
            extra.ty.arity = arity;
            s.procedures[0].args.push(extra);
        }
    };
    compatible(PING, add(TypeArity::Optional));
    breaking(PING, "the new argument `limit`", add(TypeArity::Required));
    breaking(PING, "the argument `args` was removed", |s| {
        s.procedures[0].args.clear();
    });
    breaking(PING, "the argument `args` changed", |s| {
        s.procedures[0].args[0].ty = ty("Paint");
    });
    compatible(PING, |s| {
        s.procedures[0].args[0].ty.arity = TypeArity::Optional;
    });
}

#[test]
fn reordering_existing_arguments_is_breaking() {
    let two = |names: [&str; 2]| {
        let mut s = base();
        s.procedures[0] = procedure("ping", &[(names[0], "Ping"), (names[1], "Ping")], "Ping");
        contract(&s, PING)
    };
    let verdict = classify(&two(["a", "b"]), &two(["b", "a"]));
    assert!(
        verdict
            .reasons()
            .iter()
            .any(|r| r.contains("declared order"))
    );
    assert!(classify(&two(["a", "b"]), &two(["a", "b"])).is_compatible());
}

#[test]
fn attributes_on_kept_declarations_and_fields_never_change() {
    breaking(CREATE, "gained the attribute @@paged", |s| {
        s.models[0].attributes.push(super::tests::attr("@@paged"));
    });
    breaking(CREATE, "`Widget.name` gained the attribute @default", |s| {
        s.models[0].fields[1] = with_attr(s.models[0].fields[1].clone(), "@default(\"x\")");
    });
    breaking(CREATE, "`Widget.id` lost the attribute @id", |s| {
        s.models[0].fields[0].attributes.clear();
    });
    // Dropped attributes are not in the contract at all.
    compatible(CREATE, |s| {
        s.models[0]
            .attributes
            .push(super::tests::attr("@@allow(\"read\", true)"));
        s.models[0].fields[1] = with_attr(s.models[0].fields[1].clone(), "@length(min: 1)");
    });
}

#[test]
fn transport_and_identity_changes_are_breaking() {
    let old = contract(&base(), PING);
    let mut new = old.clone();
    new["transport"] = json!("rest");
    assert!(!classify(&old, &new).is_compatible());
    let mut new = old.clone();
    new["key"] = json!("procedure.pong");
    assert!(!classify(&old, &new).is_compatible());
}

#[test]
fn a_contract_this_build_cannot_read_is_breaking() {
    let old = contract(&base(), PING);
    let mut new = old.clone();
    new["closure"]["surprise"] = json!(1);
    let verdict = classify(&old, &new);
    assert!(verdict.reasons()[0].contains("unreadable"), "{verdict:?}");
    assert!(!classify(&json!({"key": "x"}), &old).is_compatible());
}

#[test]
fn the_printed_json_is_what_the_classifier_reads() {
    let value = serde_json::from_str(&op_contract_json(&base(), PAINT).unwrap()).unwrap();
    assert!(classify(&value, &value).is_compatible());
}
