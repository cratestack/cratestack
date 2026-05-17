//! Read-only field list + edit-mode field rows rendered inside the
//! drawer.

use leptos::prelude::*;

use crate::editors::render_typed_input_optional;
use crate::types::{FieldError, FieldSummary, ModelSummary};

use super::format::{format_cell, format_value_html};

#[component]
pub fn FieldList(row: serde_json::Map<String, serde_json::Value>) -> impl IntoView {
    view! {
        <dl class="text-sm">
            {row.iter().map(|(k, v)| {
                let key = k.clone();
                let value = format_value_html(v);
                view! {
                    <div class="grid grid-cols-3 gap-2 py-1 border-b border-slate-100">
                        <dt class="text-slate-500">{key}</dt>
                        <dd class="col-span-2 font-mono text-xs break-all">{value}</dd>
                    </div>
                }
            }).collect_view()}
        </dl>
    }
}

#[component]
pub fn EditFields(
    values: ReadSignal<Option<std::collections::BTreeMap<String, String>>>,
    errors: ReadSignal<Vec<FieldError>>,
    set_values: WriteSignal<Option<std::collections::BTreeMap<String, String>>>,
    model: ModelSummary,
) -> impl IntoView {
    let writable: Vec<FieldSummary> = model
        .fields
        .iter()
        .filter(|f| !f.is_relation && f.arity != "list" && !f.is_id)
        .cloned()
        .collect();
    view! {
        <dl class="text-sm space-y-2">
            {writable.into_iter().map(|f| {
                let name = f.name.clone();
                let name_for_error = name.clone();
                let field_for_input = f.clone();
                view! {
                    <div class="grid grid-cols-3 gap-2 items-start">
                        <dt class="text-slate-500 pt-1">{name.clone()}</dt>
                        <dd class="col-span-2">
                            {render_typed_input_optional(field_for_input, values, set_values)}
                            {move || errors.get().iter()
                                .find(|e| e.field == name_for_error)
                                .map(|e| view! {
                                    <p class="text-xs text-red-700 mt-0.5">{e.message.clone()}</p>
                                }.into_any())
                                .unwrap_or_else(|| ().into_any())}
                        </dd>
                    </div>
                }
            }).collect_view()}
        </dl>
    }
}

/// Pull the primary key out of a row, formatting whatever scalar shape
/// the backend returned as a string. Used by the drawer when building
/// snippet/follow requests that include `pk` on the URL.
pub fn row_pk(row: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    row.get("id").map(|v| match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// Snapshot a row's writable fields into the edit-mode signal map.
pub fn snapshot_for_edit(
    row: &serde_json::Map<String, serde_json::Value>,
    model: &ModelSummary,
) -> std::collections::BTreeMap<String, String> {
    model
        .fields
        .iter()
        .filter(|f| !f.is_relation && f.arity != "list" && !f.is_id)
        .map(|f| {
            let v = row.get(&f.name).map(format_cell).unwrap_or_default();
            (f.name.clone(), v)
        })
        .collect()
}
