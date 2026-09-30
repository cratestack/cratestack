//! Shared IR builders for the core tests. The parsed-source invariance
//! and sensitivity suites live in `cratestack-parser/tests/op_contract_*`.

use crate::schema::{
    Attribute, Field, Model, Procedure, ProcedureKind, Schema, SourceSpan, TypeArity, TypeRef,
};

fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 1,
        line: 1,
    }
}

pub(super) fn attr(raw: &str) -> Attribute {
    Attribute {
        raw: raw.to_owned(),
        span: span(),
    }
}

pub(super) fn ty(name: &str) -> TypeRef {
    TypeRef {
        name: name.to_owned(),
        name_span: span(),
        arity: TypeArity::Required,
        generic_args: vec![],
        int_args: vec![],
        ident_args: vec![],
    }
}

pub(super) fn field(name: &str, ty_name: &str, attrs: &[&str]) -> Field {
    Field {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        ty: ty(ty_name),
        attributes: attrs.iter().map(|a| attr(a)).collect(),
        span: span(),
    }
}

pub(super) fn model(name: &str, fields: Vec<Field>, attrs: &[&str]) -> Model {
    Model {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        fields,
        attributes: attrs.iter().map(|a| attr(a)).collect(),
        span: span(),
        mcp: None,
    }
}

pub(super) fn procedure(name: &str, arg: &str, ret: &str, attrs: &[&str]) -> Procedure {
    Procedure {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        kind: ProcedureKind::Mutation,
        args: vec![crate::schema::ProcedureArg {
            docs: vec![],
            name: "args".to_owned(),
            name_span: span(),
            ty: ty(arg),
            span: span(),
        }],
        return_type: ty(ret),
        attributes: attrs.iter().map(|a| attr(a)).collect(),
        span: span(),
        mcp: None,
    }
}

pub(super) fn empty() -> Schema {
    serde_json::from_str(
        r#"{"datasource":null,"auth":null,"config_blocks":[],"mixins":[],
            "models":[],"types":[],"enums":[],"procedures":[]}"#,
    )
    .unwrap()
}

/// `model Widget { id Int @id  name String }`, `type Ping { note String }`,
/// `procedure ping(args: Ping): Ping`.
pub(super) fn sample() -> Schema {
    let mut schema = empty();
    schema.models.push(model(
        "Widget",
        vec![field("id", "Int", &["@id"]), field("name", "String", &[])],
        &[],
    ));
    schema.types.push(crate::schema::TypeDecl {
        docs: vec![],
        name: "Ping".to_owned(),
        name_span: span(),
        fields: vec![field("note", "String", &[])],
        span: span(),
    });
    schema
        .procedures
        .push(procedure("ping", "Ping", "Ping", &[]));
    schema
}
