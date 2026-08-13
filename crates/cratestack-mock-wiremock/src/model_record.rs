//! Synthesizes a single example instance of a model's default REST/RPC
//! projection, and wraps it into the `list` route's response envelope.

use std::collections::BTreeSet;

use cratestack_core::{Model, Schema};
use serde_json::{Map, Value, json};

use crate::error::WireMockGeneratorError;
use crate::model_attrs::{is_relation_field, is_server_only_field};
use crate::values::synthesize;

/// Builds a synthesized example instance of `model`'s default
/// projection — the JSON body a `get`/`create`/`update`/`delete`
/// response carries with no `include=`/`fields=` query parameters
/// applied, and the shape of one `list` item. Mirrors the field set
/// `crates/cratestack-macros/src/axum/model/serializers/
/// projection_fields.rs`'s default projection builds: every field
/// *except* relation fields (populated only via `include=<relation>`)
/// and `@server_only` fields (never serialized to a client at all).
pub(crate) fn synthesize_model_record(
    schema: &Schema,
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> Result<Value, WireMockGeneratorError> {
    let owner = format!("model `{}`", model.name);
    // Seeded with the model's own name before the field loop starts,
    // the same way `values::synthesize_object` seeds `in_progress` with
    // a composite type's name before expanding its fields — guards a
    // field type that cycles back to this exact model (relation fields
    // are already excluded below, but a plain `type` field could still
    // reach the model's name through a further level of nesting).
    let mut in_progress = vec![model.name.clone()];
    let mut object = Map::with_capacity(model.fields.len());
    for field in &model.fields {
        if is_relation_field(model_names, field) || is_server_only_field(field) {
            continue;
        }
        let value = synthesize(schema, &owner, &field.ty, &mut in_progress)?;
        object.insert(field.name.clone(), value);
    }
    Ok(Value::Object(object))
}

/// Wraps a single synthesized `record` into the same `list` response
/// envelope the real handler emits: a bare JSON array for a plain
/// model, or `{items, totalCount, pageInfo}` for an `@@paged` one
/// (`crates/cratestack-macros/src/axum/model/prep/list_logging.rs`'s
/// `list_success_tokens`) — the same envelope shape `values::synthesize`
/// already builds for a procedure returning `Page<T>`.
pub(crate) fn list_envelope(paged: bool, record: &Value) -> Value {
    if paged {
        json!({
            "items": [record],
            "totalCount": 1,
            "pageInfo": {
                "limit": Value::Null,
                "offset": Value::Null,
                "hasNextPage": false,
                "hasPreviousPage": false,
            },
        })
    } else {
        Value::Array(vec![record.clone()])
    }
}
