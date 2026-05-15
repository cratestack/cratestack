//! Top-level Studio UI: header (workspace name + target switcher),
//! left sidebar (model list), main pane (records table), right drawer
//! (selected record details + relation follow + copy snippet).
//!
//! Everything is plain Leptos CSR. State is held in signals at the
//! root; child components take props.

use leptos::prelude::*;

use crate::api;
use crate::types::{
    FieldSummary, FollowResponse, ModelSummary, Page, TargetSummary,
};

const PAGE_LIMIT: u32 = 25;

#[component]
pub fn App() -> impl IntoView {
    let (workspace_name, set_workspace_name) = signal(String::new());
    let (targets, set_targets) = signal(Vec::<TargetSummary>::new());
    let (selected_target, set_selected_target) = signal(Option::<String>::None);
    let (boot_error, set_boot_error) = signal(Option::<String>::None);

    leptos::task::spawn_local(async move {
        match api::list_targets().await {
            Ok(list) => {
                set_workspace_name.set(list.workspace);
                let first_key = list.targets.first().map(|t| t.key.clone());
                set_targets.set(list.targets);
                if let Some(k) = first_key {
                    set_selected_target.set(Some(k));
                }
            }
            Err(e) => set_boot_error.set(Some(e.message)),
        }
    });

    view! {
        <div class="min-h-screen flex flex-col">
            <Header workspace_name target_list=targets selected=selected_target set_selected=set_selected_target />
            <main class="flex-1 flex">
                {move || match boot_error.get() {
                    Some(e) => view! {
                        <div class="m-8 p-4 bg-red-50 border border-red-200 rounded text-red-800 text-sm">
                            <strong class="block mb-1">"Failed to load workspace"</strong>
                            {e}
                        </div>
                    }.into_any(),
                    None => view! {
                        <Workspace target_key=selected_target />
                    }.into_any(),
                }}
            </main>
        </div>
    }
}

#[component]
fn Header(
    workspace_name: ReadSignal<String>,
    target_list: ReadSignal<Vec<TargetSummary>>,
    selected: ReadSignal<Option<String>>,
    set_selected: WriteSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <header class="border-b border-slate-200 bg-white px-6 py-3 flex items-center gap-6">
            <div>
                <span class="text-xs uppercase tracking-wide text-slate-500">"workspace"</span>
                <div class="font-semibold text-slate-900">{move || workspace_name.get()}</div>
            </div>
            <div class="flex-1" />
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

#[component]
fn Workspace(target_key: ReadSignal<Option<String>>) -> impl IntoView {
    let (models, set_models) = signal(Vec::<ModelSummary>::new());
    let (selected_model, set_selected_model) = signal(Option::<String>::None);
    let (load_error, set_load_error) = signal(Option::<String>::None);

    Effect::new(move |_| {
        let Some(key) = target_key.get() else {
            return;
        };
        set_load_error.set(None);
        set_selected_model.set(None);
        leptos::task::spawn_local(async move {
            match api::list_models(&key).await {
                Ok(list) => {
                    let first = list.models.first().map(|m| m.name.clone());
                    set_models.set(list.models);
                    set_selected_model.set(first);
                }
                Err(e) => set_load_error.set(Some(e.message)),
            }
        });
    });

    view! {
        <div class="flex-1 flex">
            <Sidebar models selected=selected_model set_selected=set_selected_model />
            <section class="flex-1 p-6 overflow-auto">
                {move || match load_error.get() {
                    Some(e) => view! {
                        <div class="p-4 bg-red-50 border border-red-200 rounded text-red-800 text-sm">{e}</div>
                    }.into_any(),
                    None => view! {
                        <RecordsPane
                            target_key
                            models
                            selected_model
                        />
                    }.into_any(),
                }}
            </section>
        </div>
    }
}

#[component]
fn Sidebar(
    models: ReadSignal<Vec<ModelSummary>>,
    selected: ReadSignal<Option<String>>,
    set_selected: WriteSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <aside class="w-56 border-r border-slate-200 bg-white p-2">
            <div class="px-2 py-1 text-xs uppercase tracking-wide text-slate-500">"Models"</div>
            <ul class="space-y-0.5">
                {move || models.get().into_iter().map(|m| {
                    let name = m.name.clone();
                    let selected_name = name.clone();
                    let click_name = name.clone();
                    let class = move || {
                        let is_active = selected.get().as_deref() == Some(selected_name.as_str());
                        if is_active {
                            "w-full text-left px-2 py-1 rounded text-sm bg-slate-900 text-white"
                        } else {
                            "w-full text-left px-2 py-1 rounded text-sm text-slate-700 hover:bg-slate-100"
                        }
                    };
                    view! {
                        <li>
                            <button
                                class=class
                                on:click=move |_| set_selected.set(Some(click_name.clone()))
                            >
                                {name}
                            </button>
                        </li>
                    }
                }).collect_view()}
            </ul>
        </aside>
    }
}

#[component]
fn RecordsPane(
    target_key: ReadSignal<Option<String>>,
    models: ReadSignal<Vec<ModelSummary>>,
    selected_model: ReadSignal<Option<String>>,
) -> impl IntoView {
    let (page, set_page) = signal(Option::<Page>::None);
    let (load_error, set_load_error) = signal(Option::<String>::None);
    let (cursor_stack, set_cursor_stack) = signal(Vec::<Option<String>>::new());
    let (selected_row, set_selected_row) = signal(
        Option::<serde_json::Map<String, serde_json::Value>>::None,
    );

    let load = move |cursor: Option<String>| {
        let Some(target) = target_key.get() else {
            return;
        };
        let Some(model) = selected_model.get() else {
            return;
        };
        set_load_error.set(None);
        set_selected_row.set(None);
        leptos::task::spawn_local(async move {
            match api::list_records(&target, &model, cursor.as_deref(), PAGE_LIMIT).await {
                Ok(p) => set_page.set(Some(p)),
                Err(e) => set_load_error.set(Some(e.message)),
            }
        });
    };

    Effect::new(move |_| {
        let _ = target_key.get();
        let _ = selected_model.get();
        set_cursor_stack.set(vec![None]);
        load(None);
    });

    let next_page = move |_| {
        if let Some(p) = page.get() {
            if let Some(c) = p.next_cursor {
                set_cursor_stack.update(|s| s.push(Some(c.clone())));
                load(Some(c));
            }
        }
    };

    let prev_page = move |_| {
        set_cursor_stack.update(|s| {
            if s.len() > 1 {
                s.pop();
            }
        });
        let cursor = cursor_stack.get().last().cloned().flatten();
        load(cursor);
    };

    let current_model = move || {
        selected_model
            .get()
            .and_then(|name| models.get().into_iter().find(|m| m.name == name))
    };

    view! {
        <div class="flex gap-6 h-full">
            <div class="flex-1 min-w-0">
                {move || match (current_model(), load_error.get(), page.get()) {
                    (None, _, _) => view! { <p class="text-slate-500 text-sm">"Select a model."</p> }.into_any(),
                    (_, Some(e), _) => view! {
                        <div class="p-4 bg-red-50 border border-red-200 rounded text-red-800 text-sm">{e}</div>
                    }.into_any(),
                    (Some(m), None, Some(p)) => view! {
                        <Table
                            model=m
                            page=p
                            on_select=set_selected_row
                            on_next=Callback::new(next_page)
                            on_prev=Callback::new(prev_page)
                            can_prev=Signal::derive(move || cursor_stack.get().len() > 1)
                        />
                    }.into_any(),
                    _ => view! { <p class="text-slate-500 text-sm">"Loading…"</p> }.into_any(),
                }}
            </div>
            <Drawer
                target_key
                selected_model
                selected_row
            />
        </div>
    }
}

#[component]
fn Table(
    model: ModelSummary,
    page: Page,
    on_select: WriteSignal<Option<serde_json::Map<String, serde_json::Value>>>,
    on_next: Callback<()>,
    on_prev: Callback<()>,
    can_prev: Signal<bool>,
) -> impl IntoView {
    let columns: Vec<FieldSummary> = model
        .fields
        .iter()
        .filter(|f| !f.is_relation && f.arity != "list")
        .cloned()
        .collect();
    let column_headers = columns.clone();
    let column_rows = columns;
    let next_disabled = page.next_cursor.is_none();
    let rows = page.rows;

    view! {
        <div class="space-y-3">
            <div class="flex items-center gap-2 text-sm">
                <h2 class="font-semibold text-slate-900">{model.name.clone()}</h2>
                <span class="text-slate-500">"·"</span>
                <span class="text-slate-500">{rows.len()}" rows"</span>
            </div>
            <div class="overflow-x-auto border border-slate-200 rounded bg-white">
                <table class="min-w-full text-sm">
                    <thead class="bg-slate-100 text-slate-700 text-xs uppercase tracking-wide">
                        <tr>
                            {column_headers.iter().map(|c| {
                                view! { <th class="px-3 py-2 text-left font-medium">{c.name.clone()}</th> }
                            }).collect_view()}
                        </tr>
                    </thead>
                    <tbody>
                        {rows.into_iter().map(|row| {
                            let row_for_handler = row.clone();
                            let cells = column_rows.iter().map(|c| {
                                let value = row.get(&c.name).map(format_cell).unwrap_or_else(|| "—".to_owned());
                                view! { <td class="px-3 py-2 border-t border-slate-100 align-top">{value}</td> }
                            }).collect_view();
                            view! {
                                <tr
                                    class="cursor-pointer hover:bg-slate-50"
                                    on:click=move |_| on_select.set(Some(row_for_handler.clone()))
                                >
                                    {cells}
                                </tr>
                            }
                        }).collect_view()}
                    </tbody>
                </table>
            </div>
            <div class="flex items-center gap-2">
                <button
                    class="px-3 py-1 text-sm rounded border border-slate-300 bg-white disabled:opacity-40"
                    on:click=move |_| on_prev.run(())
                    disabled=move || !can_prev.get()
                >
                    "← Previous"
                </button>
                <button
                    class="px-3 py-1 text-sm rounded border border-slate-300 bg-white disabled:opacity-40"
                    on:click=move |_| on_next.run(())
                    disabled=next_disabled
                >
                    "Next →"
                </button>
            </div>
        </div>
    }
}

#[component]
fn Drawer(
    target_key: ReadSignal<Option<String>>,
    selected_model: ReadSignal<Option<String>>,
    selected_row: ReadSignal<Option<serde_json::Map<String, serde_json::Value>>>,
) -> impl IntoView {
    let (snippet, set_snippet) = signal(Option::<String>::None);
    let (snippet_status, set_snippet_status) = signal(String::new());
    let (follow_panel, set_follow_panel) = signal(Option::<FollowResult>::None);

    let copy_snippet = move |_| {
        let Some(target) = target_key.get() else { return };
        let Some(model) = selected_model.get() else { return };
        let Some(row) = selected_row.get() else { return };
        let Some(pk_value) = row
            .iter()
            .find_map(|(k, v)| if k == "id" { Some(v.clone()) } else { None })
        else {
            set_snippet_status.set("no `id` column on this row".to_owned());
            return;
        };
        let pk_str = match pk_value {
            serde_json::Value::String(s) => s,
            v => v.to_string(),
        };
        set_snippet_status.set(String::new());
        leptos::task::spawn_local(async move {
            match api::snippet(&target, &model, &pk_str).await {
                Ok(s) => {
                    set_snippet.set(Some(s.rust.clone()));
                    if let Some(win) = web_sys::window() {
                        let clipboard = win.navigator().clipboard();
                        let promise = clipboard.write_text(&s.rust);
                        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                        set_snippet_status.set("copied to clipboard".to_owned());
                    }
                }
                Err(e) => set_snippet_status.set(e.message),
            }
        });
    };

    let follow = move |field: String| {
        let Some(target) = target_key.get() else { return };
        let Some(model) = selected_model.get() else { return };
        let Some(row) = selected_row.get() else { return };
        let Some(pk_value) = row
            .iter()
            .find_map(|(k, v)| if k == "id" { Some(v.clone()) } else { None })
        else {
            return;
        };
        let pk_str = match pk_value {
            serde_json::Value::String(s) => s,
            v => v.to_string(),
        };
        let field_for_panel = field.clone();
        set_follow_panel.set(Some(FollowResult::Loading {
            field: field_for_panel.clone(),
        }));
        leptos::task::spawn_local(async move {
            let result = api::follow_relation(&target, &model, &pk_str, &field).await;
            set_follow_panel.set(Some(match result {
                Ok(r) => FollowResult::Loaded {
                    field: field_for_panel,
                    response: r,
                },
                Err(e) => FollowResult::Error {
                    field: field_for_panel,
                    message: e.message,
                },
            }));
        });
    };

    view! {
        <aside class="w-96 border-l border-slate-200 bg-white p-4 overflow-auto">
            <h3 class="font-semibold text-slate-900 mb-2">"Record"</h3>
            {move || match selected_row.get() {
                None => view! { <p class="text-slate-500 text-sm">"Select a row."</p> }.into_any(),
                Some(row) => view! {
                    <div class="space-y-4">
                        <dl class="text-sm">
                            {row.iter().map(|(k, v)| {
                                let key = k.clone();
                                let value = format_cell(v);
                                view! {
                                    <div class="grid grid-cols-3 gap-2 py-1 border-b border-slate-100">
                                        <dt class="text-slate-500">{key}</dt>
                                        <dd class="col-span-2 font-mono text-xs break-all">{value}</dd>
                                    </div>
                                }
                            }).collect_view()}
                        </dl>
                        <RelationActions
                            selected_model
                            on_follow=Callback::new(move |field| follow(field))
                        />
                        <FollowPanel panel=follow_panel />
                        <div class="pt-2 border-t border-slate-200">
                            <button
                                class="px-3 py-1 text-sm rounded bg-slate-900 text-white hover:bg-slate-700"
                                on:click=copy_snippet
                            >
                                "Copy Rust query"
                            </button>
                            <span class="ml-2 text-xs text-slate-500">{move || snippet_status.get()}</span>
                            {move || snippet.get().map(|s| view! {
                                <pre class="mt-2 p-2 bg-slate-50 border border-slate-200 rounded text-xs whitespace-pre-wrap break-all">{s}</pre>
                            }.into_any()).unwrap_or_else(|| ().into_any())}
                        </div>
                    </div>
                }.into_any(),
            }}
        </aside>
    }
}

#[component]
fn RelationActions(
    selected_model: ReadSignal<Option<String>>,
    on_follow: Callback<String>,
) -> impl IntoView {
    // We don't actually have `models` here — to keep Phase 1b minimal,
    // we expose a tiny input to type the relation field name. Drawer's
    // parent already knows the model, but threading the field list into
    // every drawer instance is more wiring than this PR needs; this UI
    // is correct but unpolished.
    let (field, set_field) = signal(String::new());
    view! {
        <div class="pt-2 border-t border-slate-200 space-y-1">
            <div class="text-xs uppercase tracking-wide text-slate-500">"Follow relation"</div>
            <div class="flex items-center gap-2">
                <input
                    type="text"
                    placeholder="relation field name"
                    class="flex-1 border border-slate-300 rounded px-2 py-1 text-sm"
                    prop:value=field
                    on:input=move |ev| set_field.set(event_target_value(&ev))
                />
                <button
                    class="px-2 py-1 text-sm rounded border border-slate-300 bg-white hover:bg-slate-100"
                    on:click=move |_| {
                        let value = field.get();
                        let _ = selected_model.get();
                        if !value.is_empty() {
                            on_follow.run(value);
                        }
                    }
                >
                    "Follow"
                </button>
            </div>
        </div>
    }
}

#[derive(Debug, Clone)]
enum FollowResult {
    Loading {
        field: String,
    },
    Loaded {
        field: String,
        response: FollowResponse,
    },
    Error {
        field: String,
        message: String,
    },
}

#[component]
fn FollowPanel(panel: ReadSignal<Option<FollowResult>>) -> impl IntoView {
    view! {
        {move || match panel.get() {
            None => ().into_any(),
            Some(FollowResult::Loading { field }) => view! {
                <div class="text-xs text-slate-500">"loading "{field}"…"</div>
            }.into_any(),
            Some(FollowResult::Error { field, message }) => view! {
                <div class="text-xs text-red-700">{field}": "{message}</div>
            }.into_any(),
            Some(FollowResult::Loaded { field, response }) => match response {
                FollowResponse::Single { row: None } => view! {
                    <div class="text-xs text-slate-500">{field}": no related row"</div>
                }.into_any(),
                FollowResponse::Single { row: Some(row) } => view! {
                    <div class="text-xs space-y-1">
                        <div class="font-medium text-slate-700">{field}":"</div>
                        <pre class="p-2 bg-slate-50 border border-slate-200 rounded font-mono break-all">
                            {serde_json::to_string_pretty(&row).unwrap_or_default()}
                        </pre>
                    </div>
                }.into_any(),
                FollowResponse::Page(page) => view! {
                    <div class="text-xs space-y-1">
                        <div class="font-medium text-slate-700">{field}": "{page.rows.len()}" rows"</div>
                        <pre class="p-2 bg-slate-50 border border-slate-200 rounded font-mono max-h-64 overflow-auto whitespace-pre-wrap break-all">
                            {serde_json::to_string_pretty(&page.rows).unwrap_or_default()}
                        </pre>
                    </div>
                }.into_any(),
            },
        }}
    }
}

fn format_cell(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "—".to_owned(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}
