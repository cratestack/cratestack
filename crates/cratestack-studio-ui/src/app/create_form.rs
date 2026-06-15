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
        <div class="card card-bordered card-compact bg-base-100 shadow-sm">
            <div class="card-body gap-3">
                <div class="flex items-center justify-between">
                    <h3 class="card-title text-base">"New "{model.name.clone()}</h3>
                    <button class="btn btn-ghost btn-xs" on:click=move |_| on_close.run(false)>"Cancel"</button>
                </div>
                <div class="grid grid-cols-2 gap-x-4 gap-y-2">
                    {writable_for_view.into_iter().map(|f| {
                        let name = f.name.clone();
                        let name_for_error = name.clone();
                        let field_for_input = f.clone();
                        view! {
                            <label class="form-control">
                                <div class="label py-1"><span class="label-text text-xs">{name.clone()}</span></div>
                                {render_typed_input(field_for_input, values, set_values)}
                                {move || errors.get().iter()
                                    .find(|e| e.field == name_for_error)
                                    .map(|e| view! {
                                        <span class="text-xs text-error mt-0.5">{e.message.clone()}</span>
                                    }.into_any())
                                    .unwrap_or_else(|| ().into_any())}
                            </label>
                        }
                    }).collect_view()}
                </div>
                {move || general_error.get().map(|e| view! {
                    <div role="alert" class="alert alert-error text-xs py-2">{e}</div>
                }.into_any()).unwrap_or_else(|| ().into_any())}
                <div class="card-actions">
                    <button
                        class="btn btn-primary btn-sm"
                        on:click=submit
                        disabled=move || submitting.get()
                    >
                        {move || if submitting.get() { "Creating…" } else { "Create" }}
                    </button>
                </div>
            </div>
        </div>
    }
}
