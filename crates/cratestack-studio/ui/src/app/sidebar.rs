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
        <aside class="w-56 border-r border-slate-200 bg-white p-2">
            <div class="px-2 py-1 text-xs uppercase tracking-wide text-slate-500">"Models"</div>
            <ul class="space-y-0.5">
                {move || models.get().into_iter().map(|m| {
                    let name = m.name.clone();
                    let selected_name = name.clone();
                    let click_name = name.clone();
                    let drift_name = name.clone();
                    let class = move || {
                        let is_active = selected.get().as_deref() == Some(selected_name.as_str());
                        if is_active {
                            "w-full flex items-center text-left px-2 py-1 rounded text-sm bg-slate-900 text-white"
                        } else {
                            "w-full flex items-center text-left px-2 py-1 rounded text-sm text-slate-700 hover:bg-slate-100"
                        }
                    };
                    view! {
                        <li>
                            <button
                                class=class
                                on:click=move |_| set_selected.set(Some(click_name.clone()))
                            >
                                <span>{name}</span>
                                {move || {
                                    let snap = drift.get();
                                    render_drift_dot(drift_status(&snap, &drift_name))
                                }}
                            </button>
                        </li>
                    }
                }).collect_view()}
            </ul>
        </aside>
    }
}
