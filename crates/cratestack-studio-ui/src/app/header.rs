//! Top-bar: brand mark + workspace name, the multi-cstack target
//! switcher, and the schema search / audit tools.

use leptos::prelude::*;

use crate::tools::{AuditButton, SearchBar};
use crate::types::TargetSummary;

use super::target_switcher::TargetSwitcher;

#[component]
pub fn Header(
    workspace_name: ReadSignal<String>,
    target_list: ReadSignal<Vec<TargetSummary>>,
    selected: ReadSignal<Option<String>>,
    set_selected: WriteSignal<Option<String>>,
) -> impl IntoView {
    let target_signal = Signal::derive(move || selected.get());
    view! {
        <header class="navbar min-h-12 gap-3 border-b border-base-300 bg-base-100 px-4 py-0 z-10">
            <div class="navbar-start gap-3">
                <span class="btn btn-square btn-primary btn-sm pointer-events-none">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
                         stroke-linecap="round" stroke-linejoin="round" class="w-5 h-5">
                        <path d="M12 3 3 7.5 12 12l9-4.5L12 3Z" />
                        <path d="m3 12 9 4.5L21 12" />
                        <path d="m3 16.5 9 4.5 9-4.5" />
                    </svg>
                </span>
                <div class="flex flex-col leading-tight">
                    <span class="text-sm font-semibold">"CrateStack Studio"</span>
                    <span class="text-xs opacity-50">{move || workspace_name.get()}</span>
                </div>
                <TargetSwitcher target_list selected set_selected />
            </div>
            <div class="navbar-end gap-2">
                <SearchBar target=target_signal />
                <AuditButton />
            </div>
        </header>
    }
}
