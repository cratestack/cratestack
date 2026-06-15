//! Inline form for creating a new record. Signals one per writable
//! field; submits to the API and bubbles back via `on_close(true)`
//! when the row inserts cleanly.

use leptos::prelude::*;

use crate::api;
use crate::editors::{build_payload, render_typed_input};
use crate::types::{FieldError, FieldSummary, ModelSummary};

#[component]
pub fn CreateForm(
    target: String,
    model: ModelSummary,
    on_close: Callback<bool>,
) -> impl IntoView {
    let writable: Vec<FieldSummary> = model
        .fields
        .iter()
        .filter(|f| !f.is_relation && f.arity != "list")
        .cloned()
        .collect();
    let initial: std::collections::BTreeMap<String, String> = writable
        .iter()
        .map(|f| (f.name.clone(), String::new()))
        .collect();
    let (values, set_values) = signal(initial);
    let (errors, set_errors) = signal(Vec::<FieldError>::new());
    let (submitting, set_submitting) = signal(false);
    let (general_error, set_general_error) = signal(Option::<String>::None);

    let model_name = model.name.clone();
    let writable_for_submit = writable.clone();
    let submit = move |_| {
        if submitting.get_untracked() {
            return;
        }
        set_submitting.set(true);
        set_errors.set(Vec::new());
        set_general_error.set(None);
        let target = target.clone();
        let model_name = model_name.clone();
        let raw_values = values.get_untracked();
        let payload = build_payload(&writable_for_submit, &raw_values);
        leptos::task::spawn_local(async move {
            match api::create_record(&target, &model_name, &payload).await {
                Ok(_) => {
                    on_close.run(true);
                }
                Err(e) => {
                    if e.code == "VALIDATION_ERROR" {
                        set_errors.set(e.fields);
                    } else {
                        set_general_error.set(Some(e.message));
                    }
                    set_submitting.set(false);
                }
            }
        });
    };

    let writable_for_view = writable;
    view! {
        <div class="p-4 border border-slate-200 rounded-xl bg-white shadow-sm space-y-3">
            <div class="flex items-center justify-between">
                <h3 class="font-semibold text-slate-900">"New "{model.name.clone()}</h3>
                <button
                    class="text-sm text-slate-400 hover:text-slate-700"
                    on:click=move |_| on_close.run(false)
                >
                    "Cancel"
                </button>
            </div>
            <div class="grid grid-cols-2 gap-x-4 gap-y-3">
                {writable_for_view.into_iter().map(|f| {
                    let name = f.name.clone();
                    let name_for_error = name.clone();
                    let field_for_input = f.clone();
                    view! {
                        <div>
                            <label class="block text-xs font-medium text-slate-500 mb-1">{name.clone()}</label>
                            {render_typed_input(field_for_input, values, set_values)}
                            {move || errors.get().iter()
                                .find(|e| e.field == name_for_error)
                                .map(|e| view! {
                                    <p class="text-xs text-rose-600 mt-0.5">{e.message.clone()}</p>
                                }.into_any())
                                .unwrap_or_else(|| ().into_any())}
                        </div>
                    }
                }).collect_view()}
            </div>
            {move || general_error.get().map(|e| view! {
                <div class="p-2.5 bg-rose-50 border border-rose-200 rounded-lg text-xs text-rose-800">{e}</div>
            }.into_any()).unwrap_or_else(|| ().into_any())}
            <div class="flex items-center gap-2 pt-1">
                <button
                    class="px-4 py-1.5 text-sm font-medium rounded-lg bg-indigo-600 text-white shadow-sm hover:bg-indigo-700 disabled:opacity-40 transition-colors"
                    on:click=submit
                    disabled=move || submitting.get()
                >
                    {move || if submitting.get() { "Creating…" } else { "Create" }}
                </button>
            </div>
        </div>
    }
}
