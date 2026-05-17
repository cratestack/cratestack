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
        <div class="pt-2 border-t border-slate-200 space-y-2">
            {move || if is_rw.get() {
                if editing.get().is_some() {
                    view! {
                        <div class="flex items-center gap-2">
                            <button class="px-3 py-1 text-sm rounded bg-slate-900 text-white hover:bg-slate-700"
                                on:click=move |_| save_edit.run(())>"Save"</button>
                            <button class="px-3 py-1 text-sm rounded border border-slate-300 hover:bg-slate-100"
                                on:click=move |_| cancel_edit.run(())>"Cancel"</button>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="flex items-center gap-2">
                            <button class="px-3 py-1 text-sm rounded border border-slate-300 hover:bg-slate-100"
                                on:click=move |_| start_edit.run(())>"Edit"</button>
                            <button class="px-3 py-1 text-sm rounded border border-red-200 text-red-700 hover:bg-red-50"
                                on:click=move |_| delete_row.run(())>"Delete"</button>
                        </div>
                    }.into_any()
                }
            } else {
                ().into_any()
            }}
            {move || action_status.get().map(|s| view! {
                <p class="text-xs text-slate-600">{s}</p>
            }.into_any()).unwrap_or_else(|| ().into_any())}
            <div>
                <button class="px-3 py-1 text-sm rounded bg-slate-900 text-white hover:bg-slate-700"
                    on:click=move |_| copy_snippet.run(())>"Copy Rust query"</button>
                <span class="ml-2 text-xs text-slate-500">{move || snippet_status.get()}</span>
                {move || snippet.get().map(|s| view! {
                    <pre class="mt-2 p-2 bg-slate-50 border border-slate-200 rounded text-xs whitespace-pre-wrap break-all">{s}</pre>
                }.into_any()).unwrap_or_else(|| ().into_any())}
            </div>
        </div>
    }
}
