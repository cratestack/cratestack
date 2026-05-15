//! Phase 4 power-user surfaces: SQL preview, drift indicators,
//! CSV/JSON export, schema search, and the audit-log overlay.

use leptos::prelude::*;

use crate::api;
use crate::types::{AuditEntry, ModelDrift, SearchHit, SqlPreview};

/// A small toolbar component rendered above the records table. Hosts:
/// - "Show SQL" → fetches and displays the SQL Studio would run
/// - "Export JSON" / "Export CSV" → links straight to the export
///   endpoint so the browser downloads the file.
#[component]
pub fn ToolsRow(
    target: Signal<Option<String>>,
    model: Signal<Option<String>>,
    pk: Signal<Option<String>>,
) -> impl IntoView {
    let (preview, set_preview) = signal(Option::<SqlPreview>::None);
    let (preview_error, set_preview_error) = signal(Option::<String>::None);
    let (op, set_op) = signal(String::from("list"));

    let target_for_load = target;
    let model_for_load = model;
    let pk_for_load = pk;
    let op_for_load = op;
    let load_preview = move |_| {
        let Some(t) = target_for_load.get() else { return };
        let Some(m) = model_for_load.get() else { return };
        let op_val = op_for_load.get();
        let pk_val = pk_for_load.get();
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

/// Per-model drift indicator rendered in the sidebar. Pulls
/// `/api/targets/:key/drift` once when the target changes and caches
/// the per-model status by name.
pub fn render_drift_dot(status: Option<&str>) -> impl IntoView + use<> {
    let (label, class) = match status {
        Some("drift") => ("⚠ drift", "ml-2 text-[10px] text-amber-800 bg-amber-100 px-1 rounded"),
        Some("missing_table") => (
            "✕ table",
            "ml-2 text-[10px] text-red-800 bg-red-100 px-1 rounded",
        ),
        Some("unsupported") => ("·", "ml-2 text-[10px] text-slate-400"),
        Some("skipped") => ("?", "ml-2 text-[10px] text-slate-400"),
        Some("ok") => ("", "hidden"),
        _ => ("", "hidden"),
    };
    if label.is_empty() {
        return view! { <span></span> }.into_any();
    }
    view! { <span class=class>{label}</span> }.into_any()
}

#[component]
pub fn SearchBar(target: Signal<Option<String>>) -> impl IntoView {
    let (query, set_query) = signal(String::new());
    let (hits, set_hits) = signal(Vec::<SearchHit>::new());
    let (open, set_open) = signal(false);

    let trigger = move || {
        let q = query.get();
        let Some(t) = target.get() else { return };
        if q.trim().is_empty() {
            set_hits.set(Vec::new());
            set_open.set(false);
            return;
        }
        leptos::task::spawn_local(async move {
            match api::schema_search(&t, &q).await {
                Ok(resp) => {
                    set_hits.set(resp.hits);
                    set_open.set(true);
                }
                Err(_) => {
                    set_hits.set(Vec::new());
                    set_open.set(false);
                }
            }
        });
    };

    view! {
        <div class="relative">
            <input
                type="search"
                class="border border-slate-300 rounded px-2 py-1 text-sm w-56 bg-white"
                placeholder="Search schema…"
                prop:value=move || query.get()
                on:input=move |ev| {
                    set_query.set(event_target_value(&ev));
                    trigger();
                }
                on:focus=move |_| {
                    if !hits.get_untracked().is_empty() {
                        set_open.set(true);
                    }
                }
                on:blur=move |_| {
                    // Defer so click handlers on the dropdown still fire.
                    let s = set_open;
                    leptos::task::spawn_local(async move {
                        gloo_timers::future::TimeoutFuture::new(120).await;
                        s.set(false);
                    });
                }
            />
            {move || if open.get() && !hits.get().is_empty() {
                view! {
                    <ul class="absolute right-0 mt-1 w-80 max-h-80 overflow-auto bg-white border border-slate-200 rounded shadow-lg text-sm z-10">
                        {hits.get().into_iter().take(30).map(|h| {
                            let header = format!(
                                "{} · {}",
                                h.kind,
                                h.model.clone().unwrap_or_default()
                            );
                            view! {
                                <li class="px-3 py-1.5 border-b border-slate-100 hover:bg-slate-50">
                                    <div class="text-xs text-slate-500">{header}</div>
                                    <div class="font-medium text-slate-800">{h.name.clone()}</div>
                                    <div class="text-xs text-slate-500">{h.detail.clone()}</div>
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any()
            } else {
                ().into_any()
            }}
        </div>
    }
}

#[component]
pub fn AuditButton() -> impl IntoView {
    let (open, set_open) = signal(false);
    let (entries, set_entries) = signal(Vec::<AuditEntry>::new());
    let (loading, set_loading) = signal(false);

    let load = move || {
        set_loading.set(true);
        leptos::task::spawn_local(async move {
            match api::audit_log(100).await {
                Ok(resp) => set_entries.set(resp.entries),
                Err(_) => set_entries.set(Vec::new()),
            }
            set_loading.set(false);
        });
    };

    view! {
        <button
            class="px-2 py-1 rounded border border-slate-300 bg-white text-sm hover:bg-slate-100"
            on:click=move |_| {
                let was = open.get();
                set_open.set(!was);
                if !was {
                    load();
                }
            }
        >
            "Audit"
        </button>
        {move || if open.get() {
            view! {
                <div class="absolute right-6 top-14 w-[28rem] max-h-[28rem] overflow-auto bg-white border border-slate-200 rounded shadow-lg z-20">
                    <div class="p-2 border-b border-slate-100 flex items-center justify-between">
                        <span class="text-sm font-semibold">"Recent writes"</span>
                        {move || if loading.get() {
                            view! { <span class="text-xs text-slate-500">"loading…"</span> }.into_any()
                        } else {
                            ().into_any()
                        }}
                    </div>
                    {move || if entries.get().is_empty() {
                        view! { <p class="p-3 text-xs text-slate-500">"No writes captured yet."</p> }.into_any()
                    } else {
                        view! {
                            <ul class="text-xs">
                                {entries.get().into_iter().map(|e| {
                                    let pk = e.pk.unwrap_or_else(|| "·".to_owned());
                                    view! {
                                        <li class="px-3 py-1.5 border-b border-slate-100 grid grid-cols-[6.5rem_3.5rem_1fr] gap-2">
                                            <span class="text-slate-500 font-mono">{e.at.clone()}</span>
                                            <span class="font-semibold">{e.op.clone()}</span>
                                            <span class="font-mono break-all">{format!("{}/{} → {}", e.target, e.model, pk)}</span>
                                        </li>
                                    }
                                }).collect_view()}
                            </ul>
                        }.into_any()
                    }}
                </div>
            }.into_any()
        } else {
            ().into_any()
        }}
    }
}

/// Helper: pull drift status by model name from a cached
/// `Vec<ModelDrift>` snapshot.
pub fn drift_status<'a>(snapshot: &'a [ModelDrift], model: &str) -> Option<&'a str> {
    snapshot
        .iter()
        .find(|d| d.model == model)
        .map(|d| d.status.as_str())
}
