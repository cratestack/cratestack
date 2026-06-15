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
            <div class="join">
                <select
                    class="select select-bordered select-sm join-item"
                    on:change=move |ev| set_op.set(event_target_value(&ev))
                >
                    <option value="list">"list"</option>
                    <option value="get">"get"</option>
                    <option value="create">"create"</option>
                    <option value="update">"update"</option>
                    <option value="delete">"delete"</option>
                </select>
                <button class="btn btn-sm join-item gap-1.5" on:click=load_preview>
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                         stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 opacity-60">
                        <path d="m18 16 4-4-4-4" /><path d="m6 8-4 4 4 4" /><path d="m14.5 4-5 16" />
                    </svg>
                    "Show SQL"
                </button>
            </div>
            <div class="flex-1" />
            <a class="btn btn-sm gap-1.5" href=move || export_href("json") target="_blank" rel="noopener">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                     stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 opacity-60">
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M7 10l5 5 5-5" /><path d="M12 15V3" />
                </svg>
                "JSON"
            </a>
            <a class="btn btn-sm gap-1.5" href=move || export_href("csv") target="_blank" rel="noopener">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                     stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 opacity-60">
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M7 10l5 5 5-5" /><path d="M12 15V3" />
                </svg>
                "CSV"
            </a>
        </div>
        {move || preview_error.get().map(|e| view! {
            <div role="alert" class="alert alert-error mt-2 text-xs py-2">{e}</div>
        }.into_any()).unwrap_or_else(|| ().into_any())}
        {move || preview.get().map(|p| view! {
            <div class="mt-2 p-3 bg-neutral text-neutral-content rounded-box text-xs space-y-1.5 shadow-sm">
                <div class="opacity-60">
                    <span class="font-medium opacity-100">"driver"</span>" · "{p.driver.clone()}
                </div>
                <pre class="font-mono whitespace-pre-wrap break-all text-success">{p.sql.clone()}</pre>
                {p.params.iter().map(|param| {
                    let label = format!("  ${} = <{}> {}", param.index, param.kind, param.binding);
                    view! { <div class="font-mono opacity-60">{label}</div> }
                }).collect_view()}
                {p.notes.clone().map(|n| view! {
                    <div class="text-warning">{n}</div>
                }.into_any()).unwrap_or_else(|| ().into_any())}
            </div>
        }.into_any()).unwrap_or_else(|| ().into_any())}
    }
}
