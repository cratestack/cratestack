//! `@default` admits an added field only where the generated code defaults
//! it: a model op's own model (its create input omits the field). Anywhere
//! else (a `type`, or a model reached as a procedure argument) the field is
//! required on decode, so an old client's message would fail (review of
//! cratestack#1132, B1).

use serde_json::Value;

use super::tests::field;
use super::tests_compat::*;
use super::{Verdict, classify};
use crate::schema::Schema;

const CREATE: &str = "model.Widget.create";
const PAINT: &str = "procedure.paint";
const STOCK: &str = "procedure.stock";

/// `base()` plus `stock(args: Widget): Int`, a model used as an argument.
fn with_stock() -> Schema {
    let mut s = base();
    s.procedures
        .push(procedure("stock", &[("args", "Widget")], "Int"));
    s
}

fn verdict(key: &str, edit: impl FnOnce(&mut Schema)) -> Verdict {
    let old = with_stock();
    let mut new = with_stock();
    edit(&mut new);
    let pair: [Value; 2] = [contract(&old, key), contract(&new, key)];
    classify(&pair[0], &pair[1])
}

fn refused(key: &str, edit: impl FnOnce(&mut Schema)) {
    let v = verdict(key, edit);
    assert!(
        v.reasons().iter().any(|r| r.contains("@default")),
        "{key}: {v:?}"
    );
}

#[test]
fn a_type_gaining_a_defaulted_required_field_is_breaking() {
    refused(PAINT, |s| {
        type_mut(s, "Paint")
            .fields
            .push(field("level", "Int", &["@default(1)"]));
    });
}

#[test]
fn a_model_used_as_an_argument_gaining_a_defaulted_field_is_breaking() {
    refused(STOCK, |s| {
        s.models[0]
            .fields
            .push(field("tier", "Int", &["@default(1)"]));
    });
}

#[test]
fn a_model_ops_own_model_may_gain_a_defaulted_field() {
    let v = verdict(CREATE, |s| {
        s.models[0]
            .fields
            .push(field("tier", "Int", &["@default(1)"]));
    });
    assert!(v.is_compatible(), "{v:?}");
}

#[test]
fn an_output_only_type_may_still_gain_a_defaulted_field() {
    let v = verdict(PAINT, |s| {
        type_mut(s, "Receipt")
            .fields
            .push(field("tax", "Int", &["@default(0)"]));
    });
    assert!(v.is_compatible(), "{v:?}");
}

/// An added `FindMany<T>?` argument: the parser accepts it, but the generated
/// `Args` field is never `Option<_>` and has no `serde(default)`, so an old
/// client that omits it fails to decode (S1 of the #1132 review).
#[test]
fn an_added_optional_find_many_argument_is_breaking() {
    let verdict = verdict("procedure.ping", |s| {
        let mut find = super::tests::ty("FindMany");
        find.generic_args.push(super::tests::ty("Widget"));
        find.arity = crate::schema::TypeArity::Optional;
        let mut extra = procedure("x", &[("page", "Int")], "Int").args.remove(0);
        extra.ty = find;
        s.procedures[0].args.push(extra);
    });
    assert!(
        verdict
            .reasons()
            .iter()
            .any(|r| r.contains("`page`") && r.contains("FindMany")),
        "{verdict:?}"
    );
}
