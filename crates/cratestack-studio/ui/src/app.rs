//! Top-level Studio UI.
//!
//! Layout:
//! - Header — workspace name + target switcher (with RO / RW badge).
//! - Left sidebar — models in the selected target.
//! - Main pane — records table for the selected model, with cursor
//!   pagination and a "+ New" button on RW targets.
//! - Right drawer — selected row's fields, a typed relation picker,
//!   per-field edit inputs (RW only), a delete action, and a "Copy
//!   Rust query" snippet button.
//!
//! State is held in signals at the root; child components take props.

use leptos::prelude::*;

use crate::api;
use crate::editors::{build_payload, render_typed_input, render_typed_input_optional};
use crate::tools::{AuditButton, SearchBar, ToolsRow, drift_status, render_drift_dot};
use crate::types::{
    FieldError, FieldSummary, FollowResponse, ModelDrift, ModelSummary, Page, TargetSummary,
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

    let current_target_mode = Signal::derive(move || {
        let key = selected_target.get()?;
        targets
            .get()
            .into_iter()
            .find(|t| t.key == key)
            .map(|t| t.mode)
    });

    view! {
        <div class="min-h-screen flex flex-col">
            <Header
                workspace_name
                target_list=targets
                selected=selected_target
                set_selected=set_selected_target
            />
            <main class="flex-1 flex">
                {move || match boot_error.get() {
                    Some(e) => view! {
                        <div class="m-8 p-4 bg-red-50 border border-red-200 rounded text-red-800 text-sm">
                            <strong class="block mb-1">"Failed to load workspace"</strong>
                            {e}
                        </div>
                    }.into_any(),
                    None => view! {
                        <Workspace target_key=selected_target target_mode=current_target_mode />
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
    let selected_for_signal = selected;
    let target_signal = Signal::derive(move || selected_for_signal.get());
    view! {
        <header class="relative border-b border-slate-200 bg-white px-6 py-3 flex items-center gap-4">
            <div>
                <span class="text-xs uppercase tracking-wide text-slate-500">"workspace"</span>
                <div class="font-semibold text-slate-900">{move || workspace_name.get()}</div>
            </div>
            <div class="flex-1" />
            <SearchBar target=target_signal />
            <AuditButton />
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
fn Workspace(
    target_key: ReadSignal<Option<String>>,
    target_mode: Signal<Option<String>>,
) -> impl IntoView {
    let (models, set_models) = signal(Vec::<ModelSummary>::new());
    let (selected_model, set_selected_model) = signal(Option::<String>::None);
    let (load_error, set_load_error) = signal(Option::<String>::None);
    let (drift, set_drift) = signal(Vec::<ModelDrift>::new());

    Effect::new(move |_| {
        let Some(key) = target_key.get() else {
            return;
        };
        set_load_error.set(None);
        set_selected_model.set(None);
        set_drift.set(Vec::new());
        let key_for_drift = key.clone();
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
        leptos::task::spawn_local(async move {
            if let Ok(resp) = api::target_drift(&key_for_drift).await {
                set_drift.set(resp.models);
            }
        });
    });

    view! {
        <div class="flex-1 flex">
            <Sidebar models selected=selected_model set_selected=set_selected_model drift=drift />
            <section class="flex-1 p-6 overflow-auto">
                {move || match load_error.get() {
                    Some(e) => view! {
                        <div class="p-4 bg-red-50 border border-red-200 rounded text-red-800 text-sm">{e}</div>
                    }.into_any(),
                    None => view! {
                        <RecordsPane
                            target_key
                            target_mode
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
    drift: ReadSignal<Vec<ModelDrift>>,
) -> impl IntoView {
    view! {
        <aside class="w-56 border-r border-slate-200 bg-white p-2">
            <div class="px-2 py-1 text-xs uppercase tracking-wide text-slate-500">"Models"</div>
            <ul class="space-y-0.5">
                {move || models.get().into_iter().map(|m| {
                    let name = m.name.clone();
                    let selected_name = name.clone();
                    let click_name = name.clone();
                    let drift_name = name.clone();
                    let class = move || {
                        let is_active = selected.get().as_deref() == Some(selected_name.as_str());
                        if is_active {
                            "w-full flex items-center text-left px-2 py-1 rounded text-sm bg-slate-900 text-white"
                        } else {
                            "w-full flex items-center text-left px-2 py-1 rounded text-sm text-slate-700 hover:bg-slate-100"
                        }
                    };
                    view! {
                        <li>
                            <button
                                class=class
                                on:click=move |_| set_selected.set(Some(click_name.clone()))
                            >
                                <span>{name}</span>
                                {move || {
                                    let snap = drift.get();
                                    render_drift_dot(drift_status(&snap, &drift_name))
                                }}
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
    target_mode: Signal<Option<String>>,
    models: ReadSignal<Vec<ModelSummary>>,
    selected_model: ReadSignal<Option<String>>,
) -> impl IntoView {
    let (page, set_page) = signal(Option::<Page>::None);
    let (load_error, set_load_error) = signal(Option::<String>::None);
    let (cursor_stack, set_cursor_stack) = signal(Vec::<Option<String>>::new());
    let (selected_row, set_selected_row) = signal(
        Option::<serde_json::Map<String, serde_json::Value>>::None,
    );
    let (creating, set_creating) = signal(false);
    let (reload_token, set_reload_token) = signal(0u32);

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
        let _ = reload_token.get();
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

    let current_model = Signal::derive(move || {
        selected_model
            .get()
            .and_then(|name| models.get().into_iter().find(|m| m.name == name))
    });

    let is_rw =
        Signal::derive(move || target_mode.get().as_deref() == Some("rw"));

    let target_signal: Signal<Option<String>> = Signal::derive(move || target_key.get());
    let model_signal: Signal<Option<String>> = Signal::derive(move || selected_model.get());
    let pk_signal: Signal<Option<String>> = Signal::derive(move || {
        selected_row.get().and_then(|row| {
            row.get("id").map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
        })
    });

    view! {
        <div class="flex gap-6 h-full">
            <div class="flex-1 min-w-0 space-y-3">
                <ToolsRow target=target_signal model=model_signal pk=pk_signal />
                <div class="flex items-center gap-2 text-sm">
                    {move || current_model.get().map(|m| view! {
                        <h2 class="font-semibold text-slate-900">{m.name.clone()}</h2>
                    }.into_any()).unwrap_or_else(|| ().into_any())}
                    <span class="text-slate-500">"·"</span>
                    {move || target_mode.get().map(|m| {
                        let upper = m.to_uppercase();
                        let class = if m == "rw" {
                            "text-xs px-1.5 py-0.5 rounded bg-emerald-100 text-emerald-800"
                        } else {
                            "text-xs px-1.5 py-0.5 rounded bg-slate-200 text-slate-700"
                        };
                        view! { <span class=class>{upper}</span> }.into_any()
                    }).unwrap_or_else(|| ().into_any())}
                    <span class="flex-1" />
                    {move || if is_rw.get() && !creating.get() {
                        view! {
                            <button
                                class="px-3 py-1 text-sm rounded bg-slate-900 text-white hover:bg-slate-700"
                                on:click=move |_| set_creating.set(true)
                            >
                                "+ New"
                            </button>
                        }.into_any()
                    } else {
                        ().into_any()
                    }}
                </div>

                {move || if creating.get() {
                    let Some(m) = current_model.get() else { return ().into_any() };
                    let Some(target) = target_key.get() else { return ().into_any() };
                    view! {
                        <CreateForm
                            target
                            model=m
                            on_close=Callback::new(move |inserted: bool| {
                                set_creating.set(false);
                                if inserted {
                                    set_reload_token.update(|n| *n = n.wrapping_add(1));
                                }
                            })
                        />
                    }.into_any()
                } else {
                    ().into_any()
                }}

                {move || match (current_model.get(), load_error.get(), page.get()) {
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
                target_mode
                current_model
                selected_row
                set_selected_row
                on_changed=Callback::new(move |_| {
                    set_reload_token.update(|n| *n = n.wrapping_add(1));
                })
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
    let row_count = rows.len();

    view! {
        <div class="space-y-3">
            <div class="text-xs text-slate-500">{row_count}" rows on this page"</div>
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
                                view! { <td class="px-3 py-2 border-t border-slate-100 align-top font-mono text-xs">{value}</td> }
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
fn CreateForm(
    target: String,
    model: ModelSummary,
    on_close: Callback<bool>,
) -> impl IntoView {
    // One signal per writable field, plus a global error list.
    let writable: Vec<FieldSummary> = model
        .fields
        .iter()
        .filter(|f| !f.is_relation && f.arity != "list")
        .cloned()
        .collect();
    let initial: std::collections::BTreeMap<String, String> = writable
        .iter()
        .map(|f| (f.name.clone(), String::new()))
        .collect();
    let (values, set_values) = signal(initial);
    let (errors, set_errors) = signal(Vec::<FieldError>::new());
    let (submitting, set_submitting) = signal(false);
    let (general_error, set_general_error) = signal(Option::<String>::None);

    let model_name = model.name.clone();
    let writable_for_submit = writable.clone();
    let submit = move |_| {
        if submitting.get_untracked() {
            return;
        }
        set_submitting.set(true);
        set_errors.set(Vec::new());
        set_general_error.set(None);
        let target = target.clone();
        let model_name = model_name.clone();
        let raw_values = values.get_untracked();
        let payload = build_payload(&writable_for_submit, &raw_values);
        leptos::task::spawn_local(async move {
            match api::create_record(&target, &model_name, &payload).await {
                Ok(_) => {
                    on_close.run(true);
                }
                Err(e) => {
                    if e.code == "VALIDATION_ERROR" {
                        set_errors.set(e.fields);
                    } else {
                        set_general_error.set(Some(e.message));
                    }
                    set_submitting.set(false);
                }
            }
        });
    };

    let writable_for_view = writable;
    view! {
        <div class="p-4 border border-slate-200 rounded bg-white space-y-3">
            <div class="flex items-center justify-between">
                <h3 class="font-semibold text-slate-900">"New "{model.name.clone()}</h3>
                <button
                    class="text-sm text-slate-500 hover:text-slate-900"
                    on:click=move |_| on_close.run(false)
                >
                    "Cancel"
                </button>
            </div>
            <div class="space-y-2">
                {writable_for_view.into_iter().map(|f| {
                    let name = f.name.clone();
                    let name_for_error = name.clone();
                    let field_for_input = f.clone();
                    view! {
                        <div>
                            <label class="block text-xs text-slate-600 mb-0.5">{name.clone()}</label>
                            {render_typed_input(field_for_input, values, set_values)}
                            {move || errors.get().iter()
                                .find(|e| e.field == name_for_error)
                                .map(|e| view! {
                                    <p class="text-xs text-red-700 mt-0.5">{e.message.clone()}</p>
                                }.into_any())
                                .unwrap_or_else(|| ().into_any())}
                        </div>
                    }
                }).collect_view()}
            </div>
            {move || general_error.get().map(|e| view! {
                <div class="p-2 bg-red-50 border border-red-200 rounded text-xs text-red-800">{e}</div>
            }.into_any()).unwrap_or_else(|| ().into_any())}
            <div class="flex items-center gap-2">
                <button
                    class="px-3 py-1 text-sm rounded bg-slate-900 text-white hover:bg-slate-700 disabled:opacity-40"
                    on:click=submit
                    disabled=move || submitting.get()
                >
                    {move || if submitting.get() { "Creating…" } else { "Create" }}
                </button>
            </div>
        </div>
    }
}

#[component]
fn Drawer(
    target_key: ReadSignal<Option<String>>,
    target_mode: Signal<Option<String>>,
    current_model: Signal<Option<ModelSummary>>,
    selected_row: ReadSignal<Option<serde_json::Map<String, serde_json::Value>>>,
    set_selected_row: WriteSignal<Option<serde_json::Map<String, serde_json::Value>>>,
    on_changed: Callback<()>,
) -> impl IntoView {
    let (snippet, set_snippet) = signal(Option::<String>::None);
    let (snippet_status, set_snippet_status) = signal(String::new());
    let (follow_panel, set_follow_panel) = signal(Option::<FollowResult>::None);
    let (relation_field, set_relation_field) = signal(String::new());
    let (edit_values, set_edit_values): (
        ReadSignal<Option<std::collections::BTreeMap<String, String>>>,
        WriteSignal<Option<std::collections::BTreeMap<String, String>>>,
    ) = signal(None);
    let (edit_errors, set_edit_errors) = signal(Vec::<FieldError>::new());
    let (action_status, set_action_status) = signal(Option::<String>::None);

    let pk_value = Signal::derive(move || {
        selected_row.get().and_then(|row| {
            row.get("id").map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
        })
    });

    let is_rw =
        Signal::derive(move || target_mode.get().as_deref() == Some("rw"));

    let copy_snippet = move |_| {
        let Some(target) = target_key.get() else { return };
        let Some(model) = current_model.get().map(|m| m.name) else { return };
        let Some(pk) = pk_value.get() else { return };
        set_snippet_status.set(String::new());
        leptos::task::spawn_local(async move {
            match api::snippet(&target, &model, &pk).await {
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

    let follow_selected = move |_| {
        let field = relation_field.get();
        if field.is_empty() {
            return;
        }
        let Some(target) = target_key.get() else { return };
        let Some(model) = current_model.get().map(|m| m.name) else { return };
        let Some(pk) = pk_value.get() else { return };
        let field_for_panel = field.clone();
        set_follow_panel.set(Some(FollowResult::Loading {
            field: field_for_panel.clone(),
        }));
        leptos::task::spawn_local(async move {
            let result = api::follow_relation(&target, &model, &pk, &field).await;
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

    let start_edit = move |_| {
        let Some(row) = selected_row.get() else { return };
        let Some(model) = current_model.get() else { return };
        let map: std::collections::BTreeMap<String, String> = model
            .fields
            .iter()
            .filter(|f| !f.is_relation && f.arity != "list" && !f.is_id)
            .map(|f| {
                let v = row.get(&f.name).map(format_cell).unwrap_or_default();
                (f.name.clone(), v)
            })
            .collect();
        set_edit_values.set(Some(map));
        set_edit_errors.set(Vec::new());
        set_action_status.set(None);
    };

    let cancel_edit = move |_| {
        set_edit_values.set(None);
        set_edit_errors.set(Vec::new());
    };

    let save_edit = move |_| {
        let Some(target) = target_key.get() else { return };
        let Some(model) = current_model.get() else { return };
        let Some(pk) = pk_value.get() else { return };
        let Some(values) = edit_values.get_untracked() else { return };
        let writable: Vec<FieldSummary> = model
            .fields
            .iter()
            .filter(|f| !f.is_relation && f.arity != "list" && !f.is_id)
            .cloned()
            .collect();
        let payload = build_payload(&writable, &values);
        set_edit_errors.set(Vec::new());
        set_action_status.set(Some("Saving…".to_owned()));
        leptos::task::spawn_local(async move {
            match api::update_record(&target, &model.name, &pk, &payload).await {
                Ok(resp) => {
                    set_selected_row.set(Some(resp.row));
                    set_edit_values.set(None);
                    set_action_status.set(Some("Saved.".to_owned()));
                    on_changed.run(());
                }
                Err(e) => {
                    if e.code == "VALIDATION_ERROR" {
                        set_edit_errors.set(e.fields);
                        set_action_status.set(None);
                    } else {
                        set_action_status.set(Some(format!("Failed: {}", e.message)));
                    }
                }
            }
        });
    };

    let delete_row = move |_| {
        let Some(target) = target_key.get() else { return };
        let Some(model) = current_model.get().map(|m| m.name) else { return };
        let Some(pk) = pk_value.get() else { return };
        // Plain confirm() — no fancy modal in Phase 3.
        if let Some(win) = web_sys::window() {
            let ok = win
                .confirm_with_message(&format!("Delete {} {}?", model, pk))
                .unwrap_or(false);
            if !ok {
                return;
            }
        }
        set_action_status.set(Some("Deleting…".to_owned()));
        leptos::task::spawn_local(async move {
            match api::delete_record(&target, &model, &pk).await {
                Ok(_) => {
                    set_selected_row.set(None);
                    set_action_status.set(Some("Deleted.".to_owned()));
                    on_changed.run(());
                }
                Err(e) => {
                    set_action_status.set(Some(format!("Failed: {}", e.message)));
                }
            }
        });
    };

    view! {
        <aside class="w-96 border-l border-slate-200 bg-white p-4 overflow-auto">
            <h3 class="font-semibold text-slate-900 mb-2">"Record"</h3>
            {move || match selected_row.get() {
                None => view! { <p class="text-slate-500 text-sm">"Select a row."</p> }.into_any(),
                Some(row) => view! {
                    <div class="space-y-4">
                        {move || match edit_values.get() {
                            None => view! {
                                <FieldList row=row.clone() />
                            }.into_any(),
                            Some(_) => view! {
                                <EditFields
                                    values=edit_values
                                    errors=edit_errors
                                    set_values=set_edit_values
                                    model=current_model.get().expect("current model present")
                                />
                            }.into_any(),
                        }}

                        <RelationPicker
                            current_model
                            selected_field=relation_field
                            set_selected_field=set_relation_field
                            on_follow=Callback::new(follow_selected)
                        />
                        <FollowPanel panel=follow_panel />

                        <div class="pt-2 border-t border-slate-200 space-y-2">
                            {move || if is_rw.get() {
                                if edit_values.get().is_some() {
                                    view! {
                                        <div class="flex items-center gap-2">
                                            <button
                                                class="px-3 py-1 text-sm rounded bg-slate-900 text-white hover:bg-slate-700"
                                                on:click=save_edit
                                            >
                                                "Save"
                                            </button>
                                            <button
                                                class="px-3 py-1 text-sm rounded border border-slate-300 hover:bg-slate-100"
                                                on:click=cancel_edit
                                            >
                                                "Cancel"
                                            </button>
                                        </div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <div class="flex items-center gap-2">
                                            <button
                                                class="px-3 py-1 text-sm rounded border border-slate-300 hover:bg-slate-100"
                                                on:click=start_edit
                                            >
                                                "Edit"
                                            </button>
                                            <button
                                                class="px-3 py-1 text-sm rounded border border-red-200 text-red-700 hover:bg-red-50"
                                                on:click=delete_row
                                            >
                                                "Delete"
                                            </button>
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
                    </div>
                }.into_any(),
            }}
        </aside>
    }
}

#[component]
fn FieldList(row: serde_json::Map<String, serde_json::Value>) -> impl IntoView {
    view! {
        <dl class="text-sm">
            {row.iter().map(|(k, v)| {
                let key = k.clone();
                let value = format_value_html(v);
                view! {
                    <div class="grid grid-cols-3 gap-2 py-1 border-b border-slate-100">
                        <dt class="text-slate-500">{key}</dt>
                        <dd class="col-span-2 font-mono text-xs break-all">{value}</dd>
                    </div>
                }
            }).collect_view()}
        </dl>
    }
}

#[component]
fn EditFields(
    values: ReadSignal<Option<std::collections::BTreeMap<String, String>>>,
    errors: ReadSignal<Vec<FieldError>>,
    set_values: WriteSignal<Option<std::collections::BTreeMap<String, String>>>,
    model: ModelSummary,
) -> impl IntoView {
    let writable: Vec<FieldSummary> = model
        .fields
        .iter()
        .filter(|f| !f.is_relation && f.arity != "list" && !f.is_id)
        .cloned()
        .collect();
    view! {
        <dl class="text-sm space-y-2">
            {writable.into_iter().map(|f| {
                let name = f.name.clone();
                let name_for_error = name.clone();
                let field_for_input = f.clone();
                view! {
                    <div class="grid grid-cols-3 gap-2 items-start">
                        <dt class="text-slate-500 pt-1">{name.clone()}</dt>
                        <dd class="col-span-2">
                            {render_typed_input_optional(field_for_input, values, set_values)}
                            {move || errors.get().iter()
                                .find(|e| e.field == name_for_error)
                                .map(|e| view! {
                                    <p class="text-xs text-red-700 mt-0.5">{e.message.clone()}</p>
                                }.into_any())
                                .unwrap_or_else(|| ().into_any())}
                        </dd>
                    </div>
                }
            }).collect_view()}
        </dl>
    }
}

#[component]
fn RelationPicker(
    current_model: Signal<Option<ModelSummary>>,
    selected_field: ReadSignal<String>,
    set_selected_field: WriteSignal<String>,
    on_follow: Callback<()>,
) -> impl IntoView {
    view! {
        {move || {
            let Some(model) = current_model.get() else { return ().into_any() };
            let relations: Vec<FieldSummary> = model
                .fields
                .iter()
                .filter(|f| f.is_relation)
                .cloned()
                .collect();
            if relations.is_empty() {
                return view! {
                    <p class="text-xs text-slate-500">"No relations on this model."</p>
                }.into_any();
            }
            view! {
                <div class="pt-2 border-t border-slate-200 space-y-1">
                    <div class="text-xs uppercase tracking-wide text-slate-500">"Follow relation"</div>
                    <div class="flex items-center gap-2">
                        <select
                            class="flex-1 border border-slate-300 rounded px-2 py-1 text-sm bg-white"
                            on:change=move |ev| set_selected_field.set(event_target_value(&ev))
                        >
                            <option value="">"Select…"</option>
                            {relations.into_iter().map(|f| {
                                let is_selected = selected_field.get() == f.name;
                                let label = format!("{} → {} ({})", f.name, f.type_name, f.arity);
                                view! {
                                    <option value=f.name.clone() selected=is_selected>{label}</option>
                                }
                            }).collect_view()}
                        </select>
                        <button
                            class="px-2 py-1 text-sm rounded border border-slate-300 bg-white hover:bg-slate-100 disabled:opacity-40"
                            on:click=move |_| on_follow.run(())
                            disabled=move || selected_field.get().is_empty()
                        >
                            "Follow"
                        </button>
                    </div>
                </div>
            }.into_any()
        }}
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

/// Pretty-print object/array cells in the drawer; scalars use the
/// short [`format_cell`] form.
fn format_value_html(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
        other => format_cell(other),
    }
}
