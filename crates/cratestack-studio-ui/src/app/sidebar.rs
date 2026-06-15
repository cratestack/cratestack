//! Left sidebar — clickable list of models in the current target, with
//! drift indicators rendered next to each entry.

use leptos::prelude::*;

use crate::tools::{drift_status, render_drift_dot};
use crate::types::{ModelDrift, ModelSummary};

#[component]
pub fn Sidebar(
    models: ReadSignal<Vec<ModelSummary>>,
    selected: ReadSignal<Option<String>>,
    set_selected: WriteSignal<Option<String>>,
    drift: ReadSignal<Vec<ModelDrift>>,
) -> impl IntoView {
    view! {
        <aside class="w-52 shrink-0 border-r border-base-300 bg-base-100 overflow-y-auto">
            <ul class="menu menu-sm gap-0.5">
                <li class="menu-title flex-row items-center justify-between">
                    <span>"Models"</span>
                    <span class="badge badge-ghost badge-sm">{move || models.get().len()}</span>
                </li>
                {move || models.with(|list| list.iter().map(|m| {
                    let name = m.name.clone();
                    let selected_name = name.clone();
                    let dot_name = name.clone();
                    let click_name = name.clone();
                    let drift_name = name.clone();
                    let class = move || {
                        let is_active = selected.get().as_deref() == Some(selected_name.as_str());
                        if is_active {
                            "group flex items-center gap-2 text-sm font-semibold active"
                        } else {
                            "group flex items-center gap-2 text-sm text-base-content/70"
                        }
                    };
                    let dot_class = move || {
                        let is_active = selected.get().as_deref() == Some(dot_name.as_str());
                        if is_active { "w-1.5 h-1.5 rounded-sm bg-primary-content shrink-0" }
                        else { "w-1.5 h-1.5 rounded-sm bg-base-content/30 group-hover:bg-base-content/50 shrink-0" }
                    };
                    view! {
                        <li>
                            <button
                                class=class
                                on:click=move |_| set_selected.set(Some(click_name.clone()))
                            >
                                <span class=dot_class />
                                <span class="flex-1 truncate">{name}</span>
                                {move || {
                                    let snap = drift.get();
                                    render_drift_dot(drift_status(&snap, &drift_name))
                                }}
                            </button>
                        </li>
                    }
                }).collect_view())}
            </ul>
        </aside>
    }
}
