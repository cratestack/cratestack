//! Shared IR builders and the op-list tests. The parsed-source invariance
//! and sensitivity suites live in `cratestack-parser/tests/op_contract_*`.

use super::*;
use crate::schema::{Attribute, Field, Model, SourceSpan, TypeRef};

fn span() -> SourceSpan {
    SourceSpan { start: 0, end: 1, line: 1 }
}

pub(super) fn attr(raw: &str) -> Attribute {
    Attribute { raw: raw.to_owned(), span: span() }
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
    schema.procedures.push(procedure("ping", "Ping", "Ping", &[]));
    schema
}

fn keys(schema: &Schema) -> Vec<String> {
    ops(schema).into_iter().map(|op| op.key).collect()
}

#[test]
fn rpc_ops_are_the_op_ids_the_macro_emits() {
    let mut schema = sample();
    schema.transport = TransportStyle::Rpc;
    assert_eq!(
        keys(&schema),
        [
            "model.Widget.create",
            "model.Widget.delete",
            "model.Widget.get",
            "model.Widget.list",
            "model.Widget.update",
            "procedure.ping",
        ]
    );
}

#[test]
fn rest_ops_are_method_and_route_template() {
    assert_eq!(
        keys(&sample()),
        [
            "DELETE /widgets/{id}",
            "GET /widgets",
            "GET /widgets/{id}",
            "PATCH /widgets/{id}",
            "POST /$procs/ping",
            "POST /widgets",
        ]
    );
}

#[test]
fn an_api_version_is_part_of_a_rest_key() {
    let mut schema = sample();
    schema.procedures[0].attributes.push(attr("@api_version(\"v2\")"));
    assert!(keys(&schema).contains(&"POST /v2/$procs/ping".to_owned()));
}

#[test]
fn internal_verbs_are_not_ops_and_subscribe_is() {
    let mut schema = sample();
    schema.transport = TransportStyle::Rpc;
    schema.models[0].attributes = vec![
        attr("@@internal(\"delete\")"),
        attr("@@subscribe"),
        attr("@@emit(created)"),
    ];
    let keys = keys(&schema);
    assert!(!keys.contains(&"model.Widget.delete".to_owned()));
    assert!(keys.contains(&"model.Widget.subscribe".to_owned()));
}

#[test]
fn the_digest_table_is_sorted_and_one_per_op() {
    let table = op_contract_digests(&sample());
    assert_eq!(table.len(), 6);
    assert!(table.windows(2).all(|w| w[0].0 < w[1].0));
    let (key, digest) = &table[0];
    assert_eq!(op_contract_digest(&sample(), key), Some(*digest));
    assert_eq!(op_contract_digest(&sample(), "GET /nope"), None);
}

#[test]
fn equal_shapes_on_different_ops_have_different_digests() {
    let table = op_contract_digests(&sample());
    let mut digests: Vec<_> = table.iter().map(|(_, d)| *d).collect();
    digests.sort();
    digests.dedup();
    assert_eq!(digests.len(), table.len());
}

#[test]
fn an_attribute_off_the_drop_list_moves_the_digest() {
    let mut schema = sample();
    let before = op_contract_digest(&schema, "GET /widgets").unwrap();
    schema.models[0].attributes.push(attr("@@someday(1)"));
    assert_ne!(before, op_contract_digest(&schema, "GET /widgets").unwrap());
}

#[test]
fn the_client_contract_moves_with_any_op() {
    let mut schema = sample();
    let before = client_contract_digest(&schema);
    schema.models[0].fields.push(field("extra", "Int", &[]));
    assert_ne!(before, client_contract_digest(&schema));
}
