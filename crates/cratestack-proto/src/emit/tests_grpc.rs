//! `transport grpc` `service` block emission — ticket #170. Kept separate
//! from `tests.rs` (ticket #169, messages/enums only, already at the
//! repo's 200-LoC file ceiling) rather than extending it further.
//!
//! Helper builders below intentionally duplicate `tests.rs`'s small IR
//! constructors rather than sharing them across a module boundary — cheap,
//! low-risk test-only duplication beats loosening `tests.rs`'s existing
//! privacy to share them.

use cratestack_core::{
    Attribute, Field, Model, Procedure, ProcedureArg, ProcedureKind, Schema, SourceSpan,
    TransportStyle, TypeArity, TypeRef,
};

use super::{emit_proto, synthesize_messages};
use crate::build_lock;

fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 0,
    }
}

fn ty(name: &str, arity: TypeArity) -> TypeRef {
    TypeRef {
        name: name.to_owned(),
        name_span: span(),
        arity,
        generic_args: vec![],
    }
}

fn field(name: &str, type_name: &str, attrs: &[&str]) -> Field {
    Field {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        ty: ty(type_name, TypeArity::Required),
        attributes: attrs
            .iter()
            .map(|raw| Attribute {
                raw: (*raw).to_owned(),
                span: span(),
            })
            .collect(),
        span: span(),
    }
}

fn model(name: &str, fields: Vec<Field>, attrs: &[&str]) -> Model {
    Model {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        fields,
        attributes: attrs
            .iter()
            .map(|raw| Attribute {
                raw: (*raw).to_owned(),
                span: span(),
            })
            .collect(),
        span: span(),
    }
}

fn procedure(name: &str, args: Vec<ProcedureArg>, return_type: TypeRef) -> Procedure {
    Procedure {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(),
        kind: ProcedureKind::Query,
        args,
        return_type,
        attributes: vec![],
        span: span(),
    }
}

fn grpc_schema() -> Schema {
    Schema {
        datasource: None,
        auth: None,
        config_blocks: vec![],
        mixins: vec![],
        models: vec![],
        types: vec![],
        enums: vec![],
        procedures: vec![],
        views: vec![],
        transport: TransportStyle::Grpc,
    }
}

fn generate(schema: &Schema) -> String {
    let extra = synthesize_messages(schema).expect("synthesize_messages");
    let mut lock = build_lock(schema, None, &extra).expect("build_lock");
    lock.package = Some("test_pkg".to_owned());
    emit_proto(schema, &lock, &extra, "schema.cstack").expect("emit_proto")
}

#[test]
fn model_with_create_policy_gets_all_five_crud_methods() {
    let schema = Schema {
        models: vec![model(
            "Order",
            vec![field("id", "Int", &["@id"]), field("total", "Int", &[])],
            &["@@allow(\"create\", true)"],
        )],
        ..grpc_schema()
    };

    let proto = generate(&schema);
    assert!(proto.contains("service Api {"));
    assert!(proto.contains("rpc ModelOrderList(OrderRpcListInput) returns (PageOfOrder);"));
    assert!(proto.contains("rpc ModelOrderGet(OrderRpcPkInput) returns (Order);"));
    assert!(proto.contains("rpc ModelOrderCreate(CreateOrderInput) returns (Order);"));
    assert!(proto.contains("rpc ModelOrderUpdate(OrderRpcUpdateInput) returns (Order);"));
    assert!(proto.contains("rpc ModelOrderDelete(OrderRpcPkInput) returns (Order);"));
}

#[test]
fn model_without_create_policy_omits_create_method_but_keeps_the_others() {
    let schema = Schema {
        models: vec![model(
            "Order",
            vec![field("id", "Int", &["@id"]), field("total", "Int", &[])],
            &[],
        )],
        ..grpc_schema()
    };

    let proto = generate(&schema);
    assert!(
        !proto.contains("ModelOrderCreate"),
        "no create-allow policy -> no create service method:\n{proto}"
    );
    assert!(
        !proto.contains("message CreateOrderInput"),
        "ticket #169's create-input gating is unchanged"
    );
    assert!(proto.contains("rpc ModelOrderList"));
    assert!(proto.contains("rpc ModelOrderGet"));
    assert!(proto.contains("rpc ModelOrderUpdate"));
    assert!(proto.contains("rpc ModelOrderDelete"));
}

#[test]
fn pk_less_model_gets_no_service_methods_but_keeps_its_message() {
    let schema = Schema {
        models: vec![model("Widget", vec![field("name", "String", &[])], &[])],
        ..grpc_schema()
    };

    let proto = generate(&schema);
    assert!(proto.contains("message Widget {"));
    assert!(
        !proto.contains("ModelWidget"),
        "a model with no primary key must get zero service methods, not just get/update/delete:\n{proto}"
    );
}

#[test]
fn sequence_procedure_returns_stream() {
    let schema = Schema {
        procedures: vec![procedure(
            "listNames",
            vec![],
            ty("String", TypeArity::List),
        )],
        ..grpc_schema()
    };

    let proto = generate(&schema);
    assert!(
        proto.contains("rpc ProcedureListNames(ListNamesInput) returns (stream ListNamesOutput);")
    );
}

#[test]
fn unary_procedure_does_not_stream() {
    let schema = Schema {
        procedures: vec![procedure(
            "getFeed",
            vec![],
            ty("String", TypeArity::Required),
        )],
        ..grpc_schema()
    };

    let proto = generate(&schema);
    assert!(proto.contains("rpc ProcedureGetFeed(GetFeedInput) returns (GetFeedOutput);"));
    assert!(!proto.contains("returns (stream GetFeedOutput)"));
}

#[test]
fn page_returning_procedure_still_synthesizes_pageof_and_shares_it_with_model_list() {
    let schema = Schema {
        models: vec![model("Order", vec![field("id", "Int", &["@id"])], &[])],
        procedures: vec![procedure(
            "listOrders",
            vec![],
            TypeRef {
                name: "Page".to_owned(),
                name_span: span(),
                arity: TypeArity::Required,
                generic_args: vec![ty("Order", TypeArity::Required)],
            },
        )],
        ..grpc_schema()
    };

    let proto = generate(&schema);
    let occurrences = proto.matches("message PageOfOrder {").count();
    assert_eq!(
        occurrences, 1,
        "PageOfOrder must be synthesized once and shared between the procedure and \
         model.Order.list:\n{proto}"
    );
    assert!(proto.contains("rpc ModelOrderList(OrderRpcListInput) returns (PageOfOrder);"));
    // The procedure's own response is `ListOrdersOutput` (ticket #169's
    // uniform one-field `result` envelope, unconditional regardless of
    // return-type shape) — `PageOfOrder` shows up one level down, as
    // `ListOrdersOutput.result`'s type, not as the rpc method's own
    // `returns (...)`.
    assert!(proto.contains("rpc ProcedureListOrders(ListOrdersInput) returns (ListOrdersOutput);"));
    assert!(proto.contains("message ListOrdersOutput {"));
    assert!(proto.contains("optional PageOfOrder result ="));
}

#[test]
fn rpc_input_messages_get_numbered_lock_entries() {
    let schema = Schema {
        models: vec![model(
            "Order",
            vec![field("id", "Cuid", &["@id"]), field("total", "Int", &[])],
            &["@@allow(\"create\", true)"],
        )],
        ..grpc_schema()
    };

    let extra = synthesize_messages(&schema).expect("synthesize_messages");
    let lock = build_lock(&schema, None, &extra).expect("build_lock");

    let pk_input = lock
        .messages
        .get("OrderRpcPkInput")
        .expect("OrderRpcPkInput lock entry");
    assert_eq!(pk_input.fields.get("id"), Some(&1));

    let update_input = lock
        .messages
        .get("OrderRpcUpdateInput")
        .expect("OrderRpcUpdateInput lock entry");
    assert_eq!(update_input.fields.get("id"), Some(&1));
    assert_eq!(update_input.fields.get("patch"), Some(&2));

    let list_input = lock
        .messages
        .get("OrderRpcListInput")
        .expect("OrderRpcListInput lock entry");
    assert_eq!(
        list_input.fields.len(),
        9,
        "limit/offset/fields/include/include_fields/sort/where_expr/or/filters"
    );

    let proto = generate(&schema);
    assert!(
        proto.contains("optional string id ="),
        "Cuid PK must map through scalar.rs like every other field:\n{proto}"
    );
    assert!(proto.contains("map<string, StringList> include_fields ="));
    assert!(
        proto.contains("optional UpdateOrderInput patch ="),
        "OrderRpcUpdateInput.patch must reference UpdateOrderInput by name:\n{proto}"
    );
}

#[test]
fn string_list_and_rpc_list_predicate_helpers_emitted_once_across_multiple_models() {
    let schema = Schema {
        models: vec![
            model("Order", vec![field("id", "Int", &["@id"])], &[]),
            model("Invoice", vec![field("id", "Int", &["@id"])], &[]),
        ],
        ..grpc_schema()
    };

    let proto = generate(&schema);
    assert_eq!(
        proto.matches("message StringList {").count(),
        1,
        "StringList must be emitted once, not per model:\n{proto}"
    );
    assert_eq!(
        proto.matches("message RpcListPredicate {").count(),
        1,
        "RpcListPredicate must be emitted once, not per model:\n{proto}"
    );
    assert!(proto.contains("message OrderRpcListInput {"));
    assert!(proto.contains("message InvoiceRpcListInput {"));
}

#[test]
fn rest_and_rpc_transports_get_no_service_block_and_no_grpc_only_messages() {
    let base_model = model(
        "Order",
        vec![field("id", "Int", &["@id"])],
        &["@@allow(\"create\", true)"],
    );

    for transport in [TransportStyle::Rest, TransportStyle::Rpc] {
        let schema = Schema {
            models: vec![base_model.clone()],
            transport,
            ..grpc_schema()
        };
        let extra = synthesize_messages(&schema).expect("synthesize_messages");
        let mut lock = build_lock(&schema, None, &extra).expect("build_lock");
        lock.package = Some("test_pkg".to_owned());
        let proto = emit_proto(&schema, &lock, &extra, "schema.cstack").expect("emit_proto");

        assert!(
            !proto.contains("service "),
            "transport {transport:?}:\n{proto}"
        );
        assert!(
            !proto.contains("OrderRpcListInput"),
            "grpc-only messages must not leak into transport {transport:?}:\n{proto}"
        );
    }
}

#[test]
fn header_states_wire_contract_for_grpc_transport() {
    let schema = grpc_schema();
    let proto = generate(&schema);
    assert!(proto.contains("`transport grpc`"));
    assert!(!proto.contains("does NOT describe the bytes on the wire"));
}
