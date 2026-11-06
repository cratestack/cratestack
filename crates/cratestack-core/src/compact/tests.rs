//! Derivation: which node each op's request and response get.

use super::tests_support::shop;
use super::{KeyNode, KeyTables, OpKeys};
use crate::op_list::{ModelVerb, model_op_key, procedure_op_key};
use crate::schema::Schema;

pub(super) fn only<'a>(tables: &'a KeyTables, op: &str) -> &'a OpKeys {
    let mut hits = tables.ops.iter().filter(|keys| keys.op == op);
    let keys = hits.next().unwrap_or_else(|| panic!("no op {op}"));
    assert!(hits.next().is_none(), "{op} listed twice");
    keys
}

pub(super) fn names(tables: &KeyTables, node: &KeyNode) -> Vec<String> {
    let KeyNode::Struct(i) = node else {
        panic!("not a struct: {node:?}");
    };
    tables.types[*i as usize]
        .fields
        .iter()
        .map(|(n, _)| n.clone())
        .collect()
}

pub(super) fn field_node<'a>(tables: &'a KeyTables, node: &KeyNode, name: &str) -> &'a KeyNode {
    let KeyNode::Struct(i) = node else {
        panic!("not a struct: {node:?}");
    };
    &tables.types[*i as usize]
        .fields
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no field {name}"))
        .1
}

fn model_op(schema: &Schema, model: &str, verb: ModelVerb) -> String {
    let rpc = schema.transport == crate::schema::TransportStyle::Rpc;
    model_op_key(model, verb, rpc).unwrap()
}

fn proc_op(schema: &Schema, name: &str) -> String {
    let rpc = schema.transport == crate::schema::TransportStyle::Rpc;
    let procedure = schema.procedures.iter().find(|p| p.name == name).unwrap();
    procedure_op_key(procedure, rpc)
}

fn check_shop(schema: &Schema) {
    let tables = KeyTables::from_schema(schema);
    assert_eq!(tables.ops.len(), crate::op_keys(schema).len());

    let create = only(&tables, &model_op(schema, "Order", ModelVerb::Create));
    // Key n is the n-th wire field by name; @server_only is gone, @computed stays.
    assert_eq!(
        names(&tables, &create.response),
        [
            "customer",
            "customerId",
            "id",
            "label",
            "lines",
            "meta",
            "note",
            "status",
            "total"
        ]
    );
    assert_eq!(create.request, create.response);
    let order = &create.response;
    // Relations point at the related struct, to-many relations are lists.
    let customer = field_node(&tables, order, "customer");
    assert_eq!(names(&tables, customer), ["home", "id", "orders", "tags"]);
    let KeyNode::List(line) = field_node(&tables, order, "lines") else {
        panic!("lines is a list");
    };
    assert_eq!(names(&tables, line), ["id", "qty"]);
    // Enums, Json, scalars and scalar lists are leaves.
    for leaf in ["status", "meta", "note", "total", "id"] {
        assert_eq!(field_node(&tables, order, leaf), &KeyNode::Opaque, "{leaf}");
    }
    assert_eq!(field_node(&tables, customer, "tags"), &KeyNode::Opaque);
    assert_eq!(
        names(&tables, field_node(&tables, customer, "home")),
        ["city", "street"]
    );

    let update = only(&tables, &model_op(schema, "Order", ModelVerb::Update));
    assert_eq!((&update.request, &update.response), (order, order));
    for verb in [ModelVerb::Get, ModelVerb::Delete] {
        let keys = only(&tables, &model_op(schema, "Order", verb));
        assert_eq!((&keys.request, &keys.response), (&KeyNode::Opaque, order));
    }
    let list = only(&tables, &model_op(schema, "Order", ModelVerb::List));
    assert_eq!(list.request, KeyNode::Opaque);
    assert_eq!(list.response, KeyNode::List(Box::new(order.clone())));
    // `@@paged` keeps the Page wrapper named.
    let paged = only(&tables, &model_op(schema, "Customer", ModelVerb::List));
    assert!(
        matches!(&paged.response, KeyNode::Page(item) if names(&tables, item) == ["home", "id", "orders", "tags"])
    );

    // A procedure's request is its Args wrapper in declaration order.
    let place = only(&tables, &proc_op(schema, "place"));
    assert_eq!(names(&tables, &place.request), ["input", "rush"]);
    assert_eq!(
        names(&tables, field_node(&tables, &place.request, "input")),
        ["city", "street"]
    );
    assert_eq!(
        field_node(&tables, &place.request, "rush"),
        &KeyNode::Opaque
    );
    assert_eq!(names(&tables, &place.response), names(&tables, order));
    let recent = only(&tables, &proc_op(schema, "recent"));
    assert_eq!(recent.request, KeyNode::Opaque);
    assert!(matches!(&recent.response, KeyNode::List(_)));
    // PageInput is a framework frame; Page<T> maps its items only.
    let browse = only(&tables, &proc_op(schema, "browse"));
    assert_eq!(
        field_node(&tables, &browse.request, "page"),
        &KeyNode::Opaque
    );
    assert!(matches!(&browse.response, KeyNode::Page(_)));
    // A @stream procedure's response is one stream item, not the list.
    let watch = only(&tables, &proc_op(schema, "watch"));
    assert_eq!(names(&tables, &watch.response), names(&tables, order));
    let ping = only(&tables, &proc_op(schema, "ping"));
    assert_eq!(ping.response, KeyNode::Opaque);
    assert_eq!(names(&tables, &ping.request), ["n"]);

    // Ops whose closures hold the same declarations share one run of structs.
    let orders = tables.types.iter().filter(|s| s.fields.len() == 9).count();
    assert_eq!(
        orders, 1,
        "Order is stored once for every op that reaches it"
    );
}

#[test]
fn rest_schema_derives_every_op() {
    check_shop(&shop("rest", false));
}

#[test]
fn rpc_schema_derives_every_op() {
    let schema = shop("rpc", false);
    assert!(crate::op_keys(&schema).contains(&"model.Order.list".to_owned()));
    check_shop(&schema);
}

#[test]
fn reordering_fields_leaves_keys_and_digests_unchanged() {
    for transport in ["rest", "rpc"] {
        let plain = KeyTables::from_schema(&shop(transport, false));
        let shuffled = KeyTables::from_schema(&shop(transport, true));
        assert_eq!(plain, shuffled);
        assert_eq!(plain.to_bytes(), shuffled.to_bytes());
    }
}

#[test]
fn find_matches_a_selector_prefix_and_digest_exactly() {
    let schema = shop("rpc", false);
    let tables = KeyTables::from_schema(&schema);
    let op = "model.Order.get";
    let digest = crate::op_contract_digest(&schema, op).unwrap();
    let selector = crate::ContractSelector::of(&digest);
    let found = tables.find(op, &selector).unwrap();
    assert_eq!(found.digest, digest);
    assert_eq!(tables.find_digest(op, &digest), Some(found));
    assert_eq!(
        tables.node(found, super::Direction::Request),
        &found.request
    );
    assert_eq!(
        tables.node(found, super::Direction::Response),
        &found.response
    );
    assert!(tables.find("model.Order.nope", &selector).is_none());
    assert!(tables.find_digest(op, &[0; 32]).is_none());
    assert!(
        tables
            .find(op, &crate::ContractSelector::of(&[0; 32]))
            .is_none()
    );
}
