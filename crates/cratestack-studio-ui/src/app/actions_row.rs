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
        <div class="pt-3 border-t border-base-300 space-y-3">
            {move || if is_rw.get() {
                if editing.get().is_some() {
                    view! {
                        <div class="flex items-center gap-2">
                            <button class="btn btn-primary btn-sm"
                                on:click=move |_| save_edit.run(())>"Save"</button>
                            <button class="btn btn-ghost btn-sm"
                                on:click=move |_| cancel_edit.run(())>"Cancel"</button>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="flex items-center gap-2">
                            <button class="btn btn-sm"
                                on:click=move |_| start_edit.run(())>"Edit"</button>
                            <button class="btn btn-sm btn-outline btn-error"
                                on:click=move |_| delete_row.run(())>"Delete"</button>
                        </div>
                    }.into_any()
                }
            } else {
                ().into_any()
            }}
            {move || action_status.get().map(|s| view! {
                <p class="text-xs text-base-content/60">{s}</p>
            }.into_any()).unwrap_or_else(|| ().into_any())}
            <div>
                <button class="btn btn-sm gap-1.5"
                    on:click=move |_| copy_snippet.run(())>
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                         stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 opacity-60">
                        <rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                    </svg>
                    "Copy Rust query"
                </button>
                <span class="ml-2 text-xs text-base-content/40">{move || snippet_status.get()}</span>
                {move || snippet.get().map(|s| view! {
                    <pre class="mt-2 p-3 bg-neutral text-success rounded-box text-xs whitespace-pre-wrap break-all shadow-sm">{s}</pre>
                }.into_any()).unwrap_or_else(|| ().into_any())}
            </div>
        </div>
    }
}
