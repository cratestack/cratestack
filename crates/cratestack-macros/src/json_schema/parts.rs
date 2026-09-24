//! Schema fragments the generator assembles: object shells, `required`
//! rules, docs, and the fixed shapes of the two `cratestack-core` structs
//! procedure I/O can name directly, `Page<T>` and `PageInput`. Split out
//! of `generator.rs` per the repo's ~200-LoC file convention.

use cratestack_core::{TypeArity, TypeRef};
use serde_json::{Map, Value, json};

use super::scalar::int;

/// Serde-derive lets a missing `Option<T>` field default to `None`, and
/// the `Bytes` fields that opt out of that with `deserialize_with` add
/// `default` back (`crate::shared::bytes_serde`). Every other arity is a
/// hard "missing field" error, and `Page`/`FindMany` are never `Option`.
pub(super) fn is_required(ty: &TypeRef) -> bool {
    ty.is_page() || ty.is_find_many() || ty.arity != TypeArity::Optional
}

/// No `additionalProperties: false`: no generated struct is
/// `deny_unknown_fields`, so serde ignores unknown keys and the schema
/// allows them too.
pub(super) fn object_schema(properties: Map<String, Value>, required: Vec<&str>) -> Value {
    let mut object = Map::new();
    object.insert("type".to_owned(), json!("object"));
    object.insert("properties".to_owned(), Value::Object(properties));
    if !required.is_empty() {
        object.insert("required".to_owned(), json!(required));
    }
    Value::Object(object)
}

/// Adds `description` from `///` docs, which agents read.
pub(super) fn with_docs(mut schema: Value, docs: &[String]) -> Value {
    let text = docs
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join("\n");
    if let (false, Some(object)) = (text.trim().is_empty(), schema.as_object_mut()) {
        object.insert("description".to_owned(), json!(text.trim()));
    }
    schema
}

pub(super) fn nullable(schema: Value) -> Value {
    json!({ "anyOf": [schema, { "type": "null" }] })
}

/// `cratestack_core::PageInput`: `rename_all = "camelCase"`, both
/// counters `Option`.
pub(super) fn page_input() -> Value {
    object_schema(counters(), Vec::new())
}

/// `cratestack_core::Page<T>` around an already-built item schema:
/// `rename_all = "camelCase"`, and `totalCount` is an `Option` with no
/// serde default, so serde_json writes it as `null` and accepts it
/// missing. `PageInfo`'s two flags are plain `bool`s, so required.
pub(super) fn page(items: Value) -> Value {
    let mut page_info = counters();
    page_info.insert("hasNextPage".to_owned(), json!({ "type": "boolean" }));
    page_info.insert("hasPreviousPage".to_owned(), json!({ "type": "boolean" }));
    let mut properties = Map::new();
    properties.insert(
        "items".to_owned(),
        json!({ "type": "array", "items": items }),
    );
    properties.insert("totalCount".to_owned(), nullable(int()));
    properties.insert(
        "pageInfo".to_owned(),
        object_schema(page_info, vec!["hasNextPage", "hasPreviousPage"]),
    );
    object_schema(properties, vec!["items", "pageInfo"])
}

/// `limit`/`offset`, shared by `PageInput` and `PageInfo`: both
/// `Option<i64>` with no serde default.
fn counters() -> Map<String, Value> {
    let mut counters = Map::new();
    counters.insert("limit".to_owned(), nullable(int()));
    counters.insert("offset".to_owned(), nullable(int()));
    counters
}
