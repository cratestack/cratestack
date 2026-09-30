//! Builders for the classifier's table: a base schema, an edit, and the
//! verdict for one op. The table itself is `tests_compat_table.rs`.

use serde_json::Value;

use super::tests::{attr, empty, field, model, ty};
use super::{Verdict, classify, op_contract_json};
use crate::schema::{
    EnumDecl, EnumVariant, ProcedureArg, Schema, SourceSpan, TransportStyle, TypeArity, TypeDecl,
};

fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 1,
        line: 1,
    }
}

fn enum_decl(name: &str, variants: &[&str]) -> EnumDecl {
    EnumDecl {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        variants: variants
            .iter()
            .map(|v| EnumVariant {
                docs: vec![],
                name: (*v).to_owned(),
                span: span(),
            })
            .collect(),
        span: span(),
    }
}

fn type_decl(name: &str, fields: Vec<crate::schema::Field>) -> TypeDecl {
    TypeDecl {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        fields,
        span: span(),
    }
}

pub(super) fn procedure(name: &str, args: &[(&str, &str)], ret: &str) -> crate::schema::Procedure {
    let mut p = super::tests::procedure(name, "Int", ret, &[]);
    p.args = args
        .iter()
        .map(|(arg, ty_name)| ProcedureArg {
            docs: vec![],
            name: (*arg).to_owned(),
            name_span: span(),
            ty: ty(ty_name),
            span: span(),
        })
        .collect();
    p
}

/// `Widget` (a model op's closure), `ping(args: Ping): Ping` (a type both
/// ways), `paint(args: Paint): Receipt` (`Paint` and enum `Shade` only go
/// in, `Receipt` and enum `Tone` only come out).
pub(super) fn base() -> Schema {
    let mut s = empty();
    s.transport = TransportStyle::Rpc;
    s.models.push(model(
        "Widget",
        vec![field("id", "Int", &["@id"]), field("name", "String", &[])],
        &[],
    ));
    s.types
        .push(type_decl("Ping", vec![field("note", "String", &[])]));
    s.types.push(type_decl(
        "Paint",
        vec![field("shade", "Shade", &[]), field("note", "String", &[])],
    ));
    s.types.push(type_decl(
        "Receipt",
        vec![field("total", "Int", &[]), field("tone", "Tone", &[])],
    ));
    s.enums.push(enum_decl("Shade", &["Light", "Dark"]));
    s.enums.push(enum_decl("Tone", &["Warm", "Cool"]));
    s.procedures
        .push(procedure("ping", &[("args", "Ping")], "Ping"));
    s.procedures
        .push(procedure("paint", &[("args", "Paint")], "Receipt"));
    s
}

pub(super) fn optional(mut f: crate::schema::Field) -> crate::schema::Field {
    f.ty.arity = TypeArity::Optional;
    f
}

pub(super) fn with_attr(mut f: crate::schema::Field, raw: &str) -> crate::schema::Field {
    f.attributes.push(attr(raw));
    f
}

pub(super) fn type_mut<'a>(s: &'a mut Schema, name: &str) -> &'a mut TypeDecl {
    s.types.iter_mut().find(|t| t.name == name).unwrap()
}

pub(super) fn enum_mut<'a>(s: &'a mut Schema, name: &str) -> &'a mut EnumDecl {
    s.enums.iter_mut().find(|e| e.name == name).unwrap()
}

pub(super) fn set_variants(s: &mut Schema, name: &str, variants: &[&str]) {
    enum_mut(s, name).variants = enum_decl(name, variants).variants;
}

pub(super) fn contract(schema: &Schema, key: &str) -> Value {
    serde_json::from_str(&op_contract_json(schema, key).expect("op exists")).unwrap()
}

/// The verdict for `key` after `edit` is applied to [`base`].
pub(super) fn after(key: &str, edit: impl FnOnce(&mut Schema)) -> Verdict {
    let old = base();
    let mut new = base();
    edit(&mut new);
    classify(&contract(&old, key), &contract(&new, key))
}

#[track_caller]
pub(super) fn compatible(key: &str, edit: impl FnOnce(&mut Schema)) {
    let verdict = after(key, edit);
    assert!(verdict.is_compatible(), "{key}: {verdict:?}");
}

/// Breaking, and some reason contains `needle`.
#[track_caller]
pub(super) fn breaking(key: &str, needle: &str, edit: impl FnOnce(&mut Schema)) {
    let verdict = after(key, edit);
    assert!(
        verdict.reasons().iter().any(|r| r.contains(needle)),
        "{key}: wanted a reason with {needle:?}, got {verdict:?}"
    );
}
