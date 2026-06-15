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
        <header class="relative z-10 border-b border-slate-200 bg-white/90 backdrop-blur px-5 h-14 flex items-center gap-4">
            <div class="flex items-center gap-2.5 pr-1">
                <BrandMark />
                <div class="flex flex-col leading-tight">
                    <span class="text-sm font-semibold text-slate-900">"CrateStack Studio"</span>
                    <span class="text-xs text-slate-400">{move || workspace_name.get()}</span>
                </div>
            </div>
            <div class="h-7 w-px bg-slate-200" />
            <TargetSwitcher target_list selected set_selected />
            <div class="flex-1" />
            <SearchBar target=target_signal />
            <AuditButton />
        </header>
    }
}

/// Stacked-crates glyph in an indigo tile — the Studio brand mark.
#[component]
fn BrandMark() -> impl IntoView {
    view! {
        <span class="flex items-center justify-center w-8 h-8 rounded-lg bg-indigo-600 text-white shadow-sm">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
                 stroke-linecap="round" stroke-linejoin="round" class="w-5 h-5">
                <path d="M12 3 3 7.5 12 12l9-4.5L12 3Z" />
                <path d="m3 12 9 4.5L21 12" />
                <path d="m3 16.5 9 4.5 9-4.5" />
            </svg>
        </span>
    }
}
