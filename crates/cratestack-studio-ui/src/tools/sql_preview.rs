//! `ToolsRow` — toolbar above the records table with SQL preview +
//! CSV / JSON export.

use leptos::prelude::*;

use crate::api;
use crate::types::SqlPreview;

#[component]
pub fn ToolsRow(
    target: Signal<Option<String>>,
    model: Signal<Option<String>>,
    pk: Signal<Option<String>>,
) -> impl IntoView {
    let (preview, set_preview) = signal(Option::<SqlPreview>::None);
    let (preview_error, set_preview_error) = signal(Option::<String>::None);
    let (op, set_op) = signal(String::from("list"));

    let load_preview = move |_| {
        let Some(t) = target.get() else { return };
        let Some(m) = model.get() else { return };
        let op_val = op.get();
        let pk_val = pk.get();
        set_preview.set(None);
        set_preview_error.set(None);
        leptos::task::spawn_local(async move {
            match api::preview_sql(&t, &m, &op_val, pk_val.as_deref()).await {
                Ok(p) => set_preview.set(Some(p)),
                Err(e) => set_preview_error.set(Some(e.message)),
            }
        });
    };

    let export_href = move |fmt: &str| {
        let t = target.get().unwrap_or_default();
        let m = model.get().unwrap_or_default();
        format!("/api/targets/{t}/models/{m}/export?format={fmt}&limit=1000")
    };

    view! {
        <div class="flex flex-wrap items-center gap-2 text-sm">
            <div class="flex items-center rounded-lg border border-slate-200 bg-white shadow-sm overflow-hidden">
                <select
                    class="px-2.5 py-1.5 bg-transparent text-slate-700 focus:outline-none border-r border-slate-200"
                    on:change=move |ev| set_op.set(event_target_value(&ev))
                >
                    <option value="list">"list"</option>
                    <option value="get">"get"</option>
                    <option value="create">"create"</option>
                    <option value="update">"update"</option>
                    <option value="delete">"delete"</option>
                </select>
                <button
                    class="flex items-center gap-1.5 px-2.5 py-1.5 font-medium text-slate-700 hover:bg-slate-50"
                    on:click=load_preview
                >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                         stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 text-slate-400">
                        <path d="m18 16 4-4-4-4" /><path d="m6 8-4 4 4 4" /><path d="m14.5 4-5 16" />
                    </svg>
                    "Show SQL"
                </button>
            </div>
            <div class="flex-1" />
            <a
                class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-slate-200 bg-white \
                       font-medium text-slate-700 shadow-sm hover:bg-slate-50 transition-colors"
                href=move || export_href("json")
                target="_blank"
                rel="noopener"
            >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                     stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 text-slate-400">
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M7 10l5 5 5-5" /><path d="M12 15V3" />
                </svg>
                "JSON"
            </a>
            <a
                class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-slate-200 bg-white \
                       font-medium text-slate-700 shadow-sm hover:bg-slate-50 transition-colors"
                href=move || export_href("csv")
                target="_blank"
                rel="noopener"
            >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                     stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 text-slate-400">
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M7 10l5 5 5-5" /><path d="M12 15V3" />
                </svg>
                "CSV"
            </a>
        </div>
        {move || preview_error.get().map(|e| view! {
            <div class="p-3 mt-2 bg-rose-50 border border-rose-200 rounded-lg text-xs text-rose-800">{e}</div>
        }.into_any()).unwrap_or_else(|| ().into_any())}
        {move || preview.get().map(|p| view! {
            <div class="mt-2 p-3 bg-slate-900 text-slate-100 border border-slate-800 rounded-xl text-xs space-y-1.5 shadow-sm">
                <div class="text-slate-400">
                    <span class="font-medium text-slate-300">"driver"</span>" · "{p.driver.clone()}
                </div>
                <pre class="font-mono whitespace-pre-wrap break-all text-emerald-300">{p.sql.clone()}</pre>
                {p.params.iter().map(|param| {
                    let label = format!("  ${} = <{}> {}", param.index, param.kind, param.binding);
                    view! { <div class="font-mono text-slate-400">{label}</div> }
                }).collect_view()}
                {p.notes.clone().map(|n| view! {
                    <div class="text-amber-300">{n}</div>
                }.into_any()).unwrap_or_else(|| ().into_any())}
            </div>
        }.into_any()).unwrap_or_else(|| ().into_any())}
    }
}
