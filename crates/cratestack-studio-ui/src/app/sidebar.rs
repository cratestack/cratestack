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
        <aside class="w-60 shrink-0 border-r border-slate-200 bg-white px-3 py-4 overflow-y-auto">
            <div class="flex items-center justify-between px-2 mb-2">
                <span class="text-[11px] font-semibold uppercase tracking-wider text-slate-400">"Models"</span>
                <span class="text-[11px] font-medium text-slate-400 bg-slate-100 rounded-full px-1.5 leading-5 min-w-5 text-center">
                    {move || models.get().len()}
                </span>
            </div>
            <ul class="space-y-0.5">
                {move || models.get().into_iter().map(|m| {
                    let name = m.name.clone();
                    let selected_name = name.clone();
                    let dot_name = name.clone();
                    let click_name = name.clone();
                    let drift_name = name.clone();
                    let class = move || {
                        let is_active = selected.get().as_deref() == Some(selected_name.as_str());
                        if is_active {
                            "group w-full flex items-center gap-2 text-left px-2 py-1.5 rounded-lg text-sm font-semibold bg-indigo-50 text-indigo-700"
                        } else {
                            "group w-full flex items-center gap-2 text-left px-2 py-1.5 rounded-lg text-sm text-slate-600 hover:bg-slate-100 hover:text-slate-900"
                        }
                    };
                    let dot_class = move || {
                        let is_active = selected.get().as_deref() == Some(dot_name.as_str());
                        if is_active { "w-1.5 h-1.5 rounded-sm bg-indigo-500 shrink-0" }
                        else { "w-1.5 h-1.5 rounded-sm bg-slate-300 group-hover:bg-slate-400 shrink-0" }
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
                }).collect_view()}
            </ul>
        </aside>
    }
}
