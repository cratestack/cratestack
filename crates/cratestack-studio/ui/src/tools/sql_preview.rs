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
        <div class="flex flex-wrap items-center gap-2 text-xs">
            <select
                class="border border-slate-300 rounded px-2 py-1 bg-white"
                on:change=move |ev| set_op.set(event_target_value(&ev))
            >
                <option value="list">"list"</option>
                <option value="get">"get"</option>
                <option value="create">"create"</option>
                <option value="update">"update"</option>
                <option value="delete">"delete"</option>
            </select>
            <button
                class="px-2 py-1 rounded border border-slate-300 bg-white hover:bg-slate-100"
                on:click=load_preview
            >
                "Show SQL"
            </button>
            <span class="text-slate-400">"|"</span>
            <a
                class="px-2 py-1 rounded border border-slate-300 bg-white hover:bg-slate-100"
                href=move || export_href("json")
                target="_blank"
                rel="noopener"
            >
                "Export JSON"
            </a>
            <a
                class="px-2 py-1 rounded border border-slate-300 bg-white hover:bg-slate-100"
                href=move || export_href("csv")
                target="_blank"
                rel="noopener"
            >
                "Export CSV"
            </a>
        </div>
        {move || preview_error.get().map(|e| view! {
            <div class="p-2 mt-2 bg-red-50 border border-red-200 rounded text-xs text-red-800">{e}</div>
        }.into_any()).unwrap_or_else(|| ().into_any())}
        {move || preview.get().map(|p| view! {
            <div class="mt-2 p-2 bg-slate-50 border border-slate-200 rounded text-xs space-y-1">
                <div class="text-slate-500">
                    <span class="font-medium">"driver:"</span>" "{p.driver.clone()}
                </div>
                <pre class="font-mono whitespace-pre-wrap break-all">{p.sql.clone()}</pre>
                {p.params.iter().map(|param| {
                    let label = format!("  ${} = <{}> {}", param.index, param.kind, param.binding);
                    view! { <div class="font-mono text-slate-600">{label}</div> }
                }).collect_view()}
                {p.notes.clone().map(|n| view! {
                    <div class="text-amber-700">{n}</div>
                }.into_any()).unwrap_or_else(|| ().into_any())}
            </div>
        }.into_any()).unwrap_or_else(|| ().into_any())}
    }
}
