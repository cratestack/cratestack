//! Bottom row of the drawer: Edit / Save / Cancel / Delete (RW only),
//! a status line, and the Copy-Rust-query button + snippet display.

use leptos::prelude::*;

pub(super) type EditValues = Option<std::collections::BTreeMap<String, String>>;

#[component]
pub fn ActionsRow(
    is_rw: Signal<bool>,
    editing: ReadSignal<EditValues>,
    start_edit: Callback<()>,
    cancel_edit: Callback<()>,
    save_edit: Callback<()>,
    delete_row: Callback<()>,
    action_status: ReadSignal<Option<String>>,
    copy_snippet: Callback<()>,
    snippet: ReadSignal<Option<String>>,
    snippet_status: ReadSignal<String>,
) -> impl IntoView {
    view! {
        <div class="pt-3 border-t border-slate-200 space-y-3">
            {move || if is_rw.get() {
                if editing.get().is_some() {
                    view! {
                        <div class="flex items-center gap-2">
                            <button class="px-3 py-1.5 text-sm font-medium rounded-lg bg-indigo-600 text-white shadow-sm hover:bg-indigo-700 transition-colors"
                                on:click=move |_| save_edit.run(())>"Save"</button>
                            <button class="px-3 py-1.5 text-sm font-medium rounded-lg border border-slate-200 bg-white text-slate-700 shadow-sm hover:bg-slate-50 transition-colors"
                                on:click=move |_| cancel_edit.run(())>"Cancel"</button>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="flex items-center gap-2">
                            <button class="px-3 py-1.5 text-sm font-medium rounded-lg border border-slate-200 bg-white text-slate-700 shadow-sm hover:bg-slate-50 transition-colors"
                                on:click=move |_| start_edit.run(())>"Edit"</button>
                            <button class="px-3 py-1.5 text-sm font-medium rounded-lg border border-rose-200 text-rose-600 bg-white shadow-sm hover:bg-rose-50 transition-colors"
                                on:click=move |_| delete_row.run(())>"Delete"</button>
                        </div>
                    }.into_any()
                }
            } else {
                ().into_any()
            }}
            {move || action_status.get().map(|s| view! {
                <p class="text-xs text-slate-500">{s}</p>
            }.into_any()).unwrap_or_else(|| ().into_any())}
            <div>
                <button class="inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg border border-slate-200 bg-white text-slate-700 shadow-sm hover:bg-slate-50 transition-colors"
                    on:click=move |_| copy_snippet.run(())>
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                         stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 text-slate-400">
                        <rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                    </svg>
                    "Copy Rust query"
                </button>
                <span class="ml-2 text-xs text-slate-400">{move || snippet_status.get()}</span>
                {move || snippet.get().map(|s| view! {
                    <pre class="mt-2 p-3 bg-slate-900 text-emerald-300 border border-slate-800 rounded-xl text-xs whitespace-pre-wrap break-all shadow-sm">{s}</pre>
                }.into_any()).unwrap_or_else(|| ().into_any())}
            </div>
        </div>
    }
}
