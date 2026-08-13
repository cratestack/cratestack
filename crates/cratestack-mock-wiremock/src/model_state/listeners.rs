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
//!
//! **`update` never touches the shared list — by design, not oversight.**
//! An earlier version had `update` overwrite the per-record context AND
//! replace its entry in the shared list (`deleteState(deleteWhere)` +
//! `recordState(addLast)`), on the theory that the list needed its own
//! up-to-date copy of every field for `list` to render correctly. That
//! non-atomic three-step sequence against one shared list context is
//! exactly what corrupted under concurrency: `wiremock-state-extension`
//! documents "single updates to contexts... are atomic on instance
//! level" but "the context can change while a request is performed" —
//! i.e. no transaction spans the three steps — and its list entries
//! "cannot be modified (only read/deleted)", so there is no atomic
//! "replace this one entry" primitive to reach for instead (checked
//! against the extension's own README before concluding this). Reproduced
//! by hand: 300+ concurrent `PATCH`es to one record left double-digit
//! duplicate stale rows for that same id in the shared list (see
//! `docs/design/wiremock-stubs.md`'s "Model CRUD statefulness" section
//! for the exact repro and counts).
//!
//! The fix removes the shared list from `update`'s write path entirely:
//! a list entry now stores only a pointer — [`fragments::
//! LIST_ENTRY_CONTEXT_KEY`], the record's own per-record context path —
//! not a denormalized copy of every field. [`super::body::list_item_body`]
//! renders each list item by following that pointer back to the
//! authoritative per-record context (the exact same lookup `get`/`delete`
//! already use), instead of reading a stale, list-local snapshot. `update`
//! therefore only ever performs the one write every other stateful field
//! mutation already relies on being atomic: `recordState` against a
//! single per-record context. `create` and `delete` still each touch the
//! shared list exactly once (an add, a delete) — the same category of
//! non-atomic multi-step risk technically still applies to a `create`/
//! `delete` race on the *same* id, but that's a materially smaller
//! window (one id is created and deleted at most once each, versus
//! `update`'s expected repeated-write pattern) and was not the reported
//! failure mode.

use serde_json::{Value, json};

use super::fields::ModelFieldPlan;
use super::fragments::{LIST_ENTRY_CONTEXT_KEY, echo_from_response};

fn state_object(plan: &ModelFieldPlan) -> Value {
    let mut object = serde_json::Map::new();
    object.insert(
        plan.pk_name.clone(),
        json!(echo_from_response(&plan.pk_name)),
    );
    for field in &plan.stateful {
        object.insert(field.name.clone(), json!(echo_from_response(&field.name)));
    }
    // Same harvest-from-response principle as every other field above:
    // `create`'s response always renders `0`, `update`'s (success-case)
    // response always renders the already-bumped value
    // (`super::body::update_body`) — this never recomputes the bump
    // itself, it just persists whatever the response already settled on.
    if let Some(version_name) = &plan.version_name {
        object.insert(
            version_name.clone(),
            json!(echo_from_response(version_name)),
        );
    }
    Value::Object(object)
}

/// [`state_object`] plus [`LIST_ENTRY_CONTEXT_KEY`] pointing back at
/// `record_context` — the shape a *list* entry stores, as opposed to a
/// per-record context's own state (which never needs to point at
/// itself).
fn list_entry_object(plan: &ModelFieldPlan, record_context: &str) -> Value {
    let Value::Object(mut object) = state_object(plan) else {
        unreachable!("state_object always returns a JSON object")
    };
    object.insert(LIST_ENTRY_CONTEXT_KEY.to_owned(), json!(record_context));
    Value::Object(object)
}

/// `POST /<plural>`: record the new row under its own per-record
/// context (`record_context`, already the fully-templated
/// `"<detail_base>/{{jsonPath response.body '$.id'}}"` string) AND
/// append a pointer to it to the shared `plural_context` list — the two
/// facts `get`/`delete` and `list` each need.
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
                "list": { "addLast": list_entry_object(plan, record_context) },
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
/// merged (patched) row. Nothing else — see this module's doc comment
/// for why the shared list is deliberately never touched here.
pub(crate) fn update_listeners(plan: &ModelFieldPlan, record_context: &str) -> Value {
    json!([
        {
            "name": "recordState",
            "parameters": { "context": record_context, "state": state_object(plan) },
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
