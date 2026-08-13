//! Builds the `serveEventListeners` arrays that persist a create/update/
//! delete into `wiremock-state-extension`'s state store — the write
//! side of the same design [`super::body`] reads back from. Every field
//! value here is harvested via `{{jsonPath response.body '$.field'}}`
//! (this response's own, already-rendered output), never recomputed
//! from `request.body` a second time — see `fragments::
//! echo_from_response`'s doc comment for why that matters for a
//! generated id specifically.
//!
//! Every `recordState`/`deleteState` "parameters" leaf is a normal JSON
//! string (never a bare/unquoted Handlebars expression) regardless of
//! the field's [`crate::model_attrs::ScalarKind`] — confirmed by hand
//! that a `"{{jsonPath response.body '$.id'}}"`-quoted leaf round-trips
//! an `Int` field correctly (the *response* still renders it unquoted;
//! only [`super::body`]'s render-time `quote_wrap` controls what the
//! client sees).

use serde_json::{Value, json};

use super::fields::ModelFieldPlan;
use super::fragments::echo_from_response;

fn state_object(plan: &ModelFieldPlan) -> Value {
    let mut object = serde_json::Map::new();
    object.insert(
        plan.pk_name.clone(),
        json!(echo_from_response(&plan.pk_name)),
    );
    for field in &plan.stateful {
        object.insert(field.name.clone(), json!(echo_from_response(&field.name)));
    }
    Value::Object(object)
}

/// `POST /<plural>`: record the new row under its own per-record
/// context (`record_context`, already the fully-templated
/// `"<detail_base>/{{jsonPath response.body '$.id'}}"` string) AND
/// append it to the shared `plural_context` list — the two facts
/// `get`/`delete` and `list` each need.
pub(crate) fn create_listeners(
    plan: &ModelFieldPlan,
    plural_context: &str,
    record_context: &str,
) -> Value {
    json!([
        {
            "name": "recordState",
            "parameters": {
                "context": plural_context,
                "list": { "addLast": state_object(plan) },
            },
        },
        {
            "name": "recordState",
            "parameters": {
                "context": record_context,
                "state": state_object(plan),
            },
        },
    ])
}

/// `PATCH /<plural>/{id}`: overwrite the per-record context with the
/// merged (patched) row, then replace that row's entry in the shared
/// list (delete-by-id, re-add) — the extension has no "update this one
/// list entry in place" operation, so removing and re-appending is the
/// only way `list` stays consistent with the per-record context.
pub(crate) fn update_listeners(
    plan: &ModelFieldPlan,
    plural_context: &str,
    record_context: &str,
) -> Value {
    json!([
        {
            "name": "recordState",
            "parameters": { "context": record_context, "state": state_object(plan) },
        },
        {
            "name": "deleteState",
            "parameters": {
                "context": plural_context,
                "list": {
                    "deleteWhere": {
                        "property": plan.pk_name,
                        "value": echo_from_response(&plan.pk_name),
                    },
                },
            },
        },
        {
            "name": "recordState",
            "parameters": {
                "context": plural_context,
                "list": { "addLast": state_object(plan) },
            },
        },
    ])
}

/// `DELETE /<plural>/{id}`: drop the per-record context, and remove the
/// matching entry from the shared list by id (extracted from this same
/// response's already-rendered — pre-delete — body, not from the
/// context this same listener array is about to delete).
pub(crate) fn delete_listeners(
    plan: &ModelFieldPlan,
    plural_context: &str,
    record_context: &str,
) -> Value {
    json!([
        { "name": "deleteState", "parameters": { "context": record_context } },
        {
            "name": "deleteState",
            "parameters": {
                "context": plural_context,
                "list": {
                    "deleteWhere": {
                        "property": plan.pk_name,
                        "value": echo_from_response(&plan.pk_name),
                    },
                },
            },
        },
    ])
}
