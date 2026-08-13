//! Assembles the full response-body template string for each verb from
//! a [`ModelFieldPlan`], using the fragment builders in [`super::
//! fragments`]. Every object/array is written with padding spaces
//! around braces/brackets (`{ "a": 1 }`, not `{"a":1}`) — not a style
//! choice, a correctness one: a bare (unquoted) numeric/boolean
//! Handlebars expression immediately followed by `}` produces `}}}`,
//! which handlebars.java's parser rejects as ambiguous (confirmed by
//! hand against the real extension); a single space breaks the
//! ambiguity and is invisible in the rendered JSON.

use super::fields::ModelFieldPlan;
use super::fragments::{
    LIST_ENTRY_CONTEXT_KEY, id_generator, merge_or_fallback, quote_wrap, read_state,
};

fn assemble_object(pairs: &[(String, String)]) -> String {
    let body = pairs
        .iter()
        .map(|(key, value)| format!("\"{key}\": {value} "))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {body}}}")
}

fn frozen_pairs(plan: &ModelFieldPlan) -> Vec<(String, String)> {
    plan.frozen
        .iter()
        .map(|field| (field.name.clone(), field.literal_json.clone()))
        .collect()
}

/// `POST /<plural>` response body: a freshly generated id, every
/// stateful field echoed from the request body if present (else its
/// static default), every frozen field at its static default.
pub(crate) fn create_body(plan: &ModelFieldPlan) -> String {
    let mut pairs = vec![(
        plan.pk_name.clone(),
        quote_wrap(
            plan.pk_kind,
            &id_generator(&plan.pk_type_name, plan.pk_kind),
        ),
    )];
    for field in &plan.stateful {
        let value = quote_wrap(
            field.kind,
            &merge_or_fallback(&field.name, &quote_wrap_default(field)),
        );
        pairs.push((field.name.clone(), value));
    }
    pairs.extend(frozen_pairs(plan));
    assemble_object(&pairs)
}

fn quote_wrap_default(field: &super::fields::StateField) -> String {
    // `merge_or_fallback`'s `{{else}}` branch is spliced directly into
    // the surrounding `quote_wrap` call in `create_body`, so the
    // fallback text itself must be bare (unquoted) — `quote_wrap` only
    // wraps the *whole* merged expression once, not each branch
    // separately. A `QuotedString` field's inner default text
    // (`string`) is therefore used as-is, not re-wrapped here.
    field.default_literal.clone()
}

/// `GET`/`DELETE` detail-route response body: every stateful field read
/// back from `context_expr`'s stored state, every frozen field at its
/// static default. Same shape for both verbs — `DELETE`'s response is
/// the pre-delete snapshot, read before the `serveEventListeners` that
/// remove it run.
pub(crate) fn read_body(plan: &ModelFieldPlan, context_expr: &str) -> String {
    let mut pairs = vec![(
        plan.pk_name.clone(),
        quote_wrap(plan.pk_kind, &read_state(&plan.pk_name, context_expr)),
    )];
    for field in &plan.stateful {
        let value = quote_wrap(field.kind, &read_state(&field.name, context_expr));
        pairs.push((field.name.clone(), value));
    }
    pairs.extend(frozen_pairs(plan));
    assemble_object(&pairs)
}

/// `PATCH` detail-route response body: the id is never patchable (always
/// re-echoed from prior state); every other stateful field takes the
/// patch body's value if present, else its prior stored value; frozen
/// fields stay at their static default (never stateful, so "patching"
/// one is a no-op the mock can't reflect either way).
pub(crate) fn update_body(plan: &ModelFieldPlan, context_expr: &str) -> String {
    let mut pairs = vec![(
        plan.pk_name.clone(),
        quote_wrap(plan.pk_kind, &read_state(&plan.pk_name, context_expr)),
    )];
    for field in &plan.stateful {
        let fallback = read_state(&field.name, context_expr);
        let value = quote_wrap(field.kind, &merge_or_fallback(&field.name, &fallback));
        pairs.push((field.name.clone(), value));
    }
    pairs.extend(frozen_pairs(plan));
    assemble_object(&pairs)
}

/// One `list` item. A list entry is a pointer, not a denormalized copy
/// of the record's fields (see `super::listeners`' module doc for why:
/// keeping the shared list's own copy in sync on every `update` is
/// exactly the non-atomic multi-step write that corrupted under
/// concurrency) — so every field, including the id, is read back from
/// the current `{{#each}}` loop item's [`LIST_ENTRY_CONTEXT_KEY`]
/// pointer via a `context=` lookup, identical to [`read_body`]'s own
/// per-record read, just against `this.<pointer key>` instead of a
/// fixed `context_expr` string.
pub(crate) fn list_item_body(plan: &ModelFieldPlan) -> String {
    read_body(plan, &format!("this.{LIST_ENTRY_CONTEXT_KEY}"))
}

/// `GET /<plural>` full response body: every stored record in
/// `plural_context`'s `list`, each rendered via [`list_item_body`], and
/// (for an `@@paged` model) wrapped in the same `{items, totalCount,
/// pageInfo}` envelope the static generator already used for a
/// procedure's `Page<T>` return type.
pub(crate) fn list_body(plan: &ModelFieldPlan, plural_context: &str, paged: bool) -> String {
    let each = format!(
        "{{{{#each (state context='{plural_context}' property='list' default='[]')}}}}{{{{#if @index}}}}, {{{{/if}}}}{} {{{{/each}}}}",
        list_item_body(plan)
    );
    let items = format!("[ {each}]");
    if !paged {
        return items;
    }
    let total =
        format!("{{{{size (state context='{plural_context}' property='list' default='[]')}}}}");
    format!(
        "{{ \"items\": {items}, \"totalCount\": {total} , \"pageInfo\": {{ \"limit\": null, \"offset\": null, \"hasNextPage\": false, \"hasPreviousPage\": false }} }}"
    )
}
