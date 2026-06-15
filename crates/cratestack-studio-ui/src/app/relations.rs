//! Relation-picker + follow-result panel rendered inside the drawer.

use leptos::prelude::*;

use crate::types::{FieldSummary, FollowResponse, ModelSummary};

#[component]
pub fn RelationPicker(
    current_model: Signal<Option<ModelSummary>>,
    selected_field: ReadSignal<String>,
    set_selected_field: WriteSignal<String>,
    on_follow: Callback<()>,
) -> impl IntoView {
    view! {
        {move || {
            let Some(model) = current_model.get() else { return ().into_any() };
            let relations: Vec<FieldSummary> = model
                .fields
                .iter()
                .filter(|f| f.is_relation)
                .cloned()
                .collect();
            if relations.is_empty() {
                return view! {
                    <p class="text-xs text-base-content/50">"No relations on this model."</p>
                }.into_any();
            }
            view! {
                <div class="pt-3 border-t border-base-300 space-y-1.5">
                    <div class="text-xs font-semibold uppercase tracking-wider text-base-content/40">"Follow relation"</div>
                    <div class="flex items-center gap-2">
                        <select
                            class="select select-bordered select-sm flex-1"
                            on:change=move |ev| set_selected_field.set(event_target_value(&ev))
                        >
                            <option value="">"Select…"</option>
                            {relations.into_iter().map(|f| {
                                let is_selected = selected_field.get() == f.name;
                                let label = format!("{} → {} ({})", f.name, f.type_name, f.arity);
                                view! {
                                    <option value=f.name.clone() selected=is_selected>{label}</option>
                                }
                            }).collect_view()}
                        </select>
                        <button
                            class="btn btn-sm"
                            on:click=move |_| on_follow.run(())
                            disabled=move || selected_field.get().is_empty()
                        >
                            "Follow"
                        </button>
                    </div>
                </div>
            }.into_any()
        }}
    }
}

#[derive(Debug, Clone)]
pub enum FollowResult {
    Loading {
        field: String,
    },
    Loaded {
        field: String,
        response: FollowResponse,
    },
    Error {
        field: String,
        message: String,
    },
}

#[component]
pub fn FollowPanel(panel: ReadSignal<Option<FollowResult>>) -> impl IntoView {
    view! {
        {move || match panel.get() {
            None => ().into_any(),
            Some(FollowResult::Loading { field }) => view! {
                <div class="text-xs text-base-content/50">"loading "{field}"…"</div>
            }.into_any(),
            Some(FollowResult::Error { field, message }) => view! {
                <div class="text-xs text-error">{field}": "{message}</div>
            }.into_any(),
            Some(FollowResult::Loaded { field, response }) => match response {
                FollowResponse::Single { row: None } => view! {
                    <div class="text-xs text-base-content/50">{field}": no related row"</div>
                }.into_any(),
                FollowResponse::Single { row: Some(row) } => view! {
                    <div class="text-xs space-y-1">
                        <div class="font-medium text-base-content/70">{field}":"</div>
                        <pre class="p-2 bg-base-200 border border-base-300 rounded-box font-mono break-all">
                            {serde_json::to_string_pretty(&row).unwrap_or_default()}
                        </pre>
                    </div>
                }.into_any(),
                FollowResponse::Page(page) => view! {
                    <div class="text-xs space-y-1">
                        <div class="font-medium text-base-content/70">{field}": "{page.rows.len()}" rows"</div>
                        <pre class="p-2 bg-base-200 border border-base-300 rounded-box font-mono max-h-64 overflow-auto whitespace-pre-wrap break-all">
                            {serde_json::to_string_pretty(&page.rows).unwrap_or_default()}
                        </pre>
                    </div>
                }.into_any(),
            },
        }}
    }
}
