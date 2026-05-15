//! Top-bar with workspace name + target switcher.

use leptos::prelude::*;

use crate::tools::{AuditButton, SearchBar};
use crate::types::TargetSummary;

#[component]
pub fn Header(
    workspace_name: ReadSignal<String>,
    target_list: ReadSignal<Vec<TargetSummary>>,
    selected: ReadSignal<Option<String>>,
    set_selected: WriteSignal<Option<String>>,
) -> impl IntoView {
    let target_signal = Signal::derive(move || selected.get());
    view! {
        <header class="relative border-b border-slate-200 bg-white px-6 py-3 flex items-center gap-4">
            <div>
                <span class="text-xs uppercase tracking-wide text-slate-500">"workspace"</span>
                <div class="font-semibold text-slate-900">{move || workspace_name.get()}</div>
            </div>
            <div class="flex-1" />
            <SearchBar target=target_signal />
            <AuditButton />
            <label class="flex items-center gap-2 text-sm text-slate-600">
                "Target"
                <select
                    class="border border-slate-300 rounded px-2 py-1 text-sm bg-white"
                    on:change=move |ev| {
                        let value = event_target_value(&ev);
                        set_selected.set(Some(value));
                    }
                >
                    {move || target_list.get().into_iter().map(|t| {
                        let is_selected = selected.get().as_deref() == Some(t.key.as_str());
                        let label = format!(
                            "{} ({} · {})",
                            t.display_name,
                            t.mode,
                            if t.has_db { "db" } else { "api" },
                        );
                        view! {
                            <option value=t.key.clone() selected=is_selected>{label}</option>
                        }
                    }).collect_view()}
                </select>
            </label>
        </header>
    }
}
