//! Schema builders for the compact tests: JSON in the IR's serde shape.

use serde_json::{Value, json};

use crate::schema::Schema;

fn span() -> Value {
    json!({"start": 0, "end": 1, "line": 1})
}

/// `"Uuid"`, `"Uuid?"`, `"Line[]"`, `"Page<Order>"`.
pub(super) fn ty(spec: &str) -> Value {
    let (rest, arity) = if let Some(rest) = spec.strip_suffix('?') {
        (rest, "Optional")
    } else if let Some(rest) = spec.strip_suffix("[]") {
        (rest, "List")
    } else {
        (spec, "Required")
    };
    let (name, generic) = match rest.split_once('<') {
        Some((name, inner)) => (name, vec![ty(inner.trim_end_matches('>'))]),
        None => (rest, vec![]),
    };
    json!({
        "name": name, "name_span": span(), "arity": arity,
        "generic_args": generic, "int_args": [], "ident_args": [],
    })
}

fn attrs(raw: &[&str]) -> Vec<Value> {
    raw.iter()
        .map(|a| json!({"raw": a, "span": span()}))
        .collect()
}

pub(super) fn field(name: &str, spec: &str, attributes: &[&str]) -> Value {
    json!({
        "docs": [], "name": name, "name_span": span(), "ty": ty(spec),
        "attributes": attrs(attributes), "span": span(),
    })
}

pub(super) fn model(name: &str, fields: Vec<Value>, attributes: &[&str]) -> Value {
    json!({
        "docs": [], "name": name, "name_span": span(), "fields": fields,
        "attributes": attrs(attributes), "span": span(),
    })
}

pub(super) fn type_decl(name: &str, fields: Vec<Value>) -> Value {
    json!({"docs": [], "name": name, "name_span": span(), "fields": fields, "span": span()})
}

pub(super) fn enum_decl(name: &str, variants: &[&str]) -> Value {
    let variants: Vec<Value> = variants
        .iter()
        .map(|v| json!({"docs": [], "name": v, "ordinal": null, "span": span()}))
        .collect();
    json!({"docs": [], "name": name, "name_span": span(), "variants": variants, "span": span()})
}

pub(super) fn procedure(
    name: &str,
    args: &[(&str, &str)],
    ret: &str,
    attributes: &[&str],
) -> Value {
    let args: Vec<Value> = args
        .iter()
        .map(|(n, t)| json!({"docs": [], "name": n, "name_span": span(), "ty": ty(t), "span": span()}))
        .collect();
    json!({
        "docs": [], "name": name, "name_span": span(), "kind": "Mutation", "args": args,
        "return_type": ty(ret), "attributes": attrs(attributes), "span": span(),
    })
}

pub(super) struct Parts {
    pub(super) transport: &'static str,
    pub(super) models: Vec<Value>,
    pub(super) types: Vec<Value>,
    pub(super) enums: Vec<Value>,
    pub(super) procedures: Vec<Value>,
}

pub(super) fn schema(parts: Parts) -> Schema {
    serde_json::from_value(json!({
        "datasource": null, "auth": null, "config_blocks": [], "mixins": [],
        "transport": parts.transport, "models": parts.models, "types": parts.types,
        "enums": parts.enums, "procedures": parts.procedures,
    }))
    .expect("the IR shape above deserializes")
}

/// A shop: orders point at customers and lines, `Customer` is `@@paged`.
/// `shuffled` lists every declaration's fields in a different order.
pub(super) fn shop(transport: &'static str, shuffled: bool) -> Schema {
    let mut order = vec![
        field("id", "Uuid", &["@id"]),
        field("total", "Decimal", &[]),
        field("status", "Status", &[]),
        field("note", "String?", &[]),
        field("meta", "Json", &[]),
        field("customerId", "Uuid", &[]),
        field(
            "customer",
            "Customer",
            &["@relation(fields: [customerId], references: [id])"],
        ),
        field("lines", "Line[]", &[]),
        field("secret", "String", &["@server_only"]),
        field("label", "String", &["@computed"]),
    ];
    let mut address = vec![
        field("street", "String", &[]),
        field("city", "String?", &[]),
    ];
    if shuffled {
        order.reverse();
        address.reverse();
    }
    schema(Parts {
        transport,
        models: vec![
            model("Order", order, &[]),
            model(
                "Customer",
                vec![
                    field("id", "Uuid", &["@id"]),
                    field("home", "Address", &[]),
                    field("tags", "String[]", &[]),
                    field("orders", "Order[]", &[]),
                ],
                &["@@paged"],
            ),
            model(
                "Line",
                vec![field("id", "Uuid", &["@id"]), field("qty", "Int", &[])],
                &[],
            ),
        ],
        types: vec![type_decl("Address", address)],
        enums: vec![enum_decl("Status", &["Open", "Paid"])],
        procedures: vec![
            procedure(
                "place",
                &[("input", "Address"), ("rush", "Boolean?")],
                "Order",
                &[],
            ),
            procedure("recent", &[], "Order[]", &[]),
            procedure("browse", &[("page", "PageInput")], "Page<Customer>", &[]),
            procedure("watch", &[("id", "Uuid")], "Order[]", &["@stream"]),
            procedure("ping", &[("n", "Int")], "Int", &[]),
        ],
    })
}
