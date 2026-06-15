//! Center pane: tools row + records table + "+ New" form, plus the
//! signals that wire pagination and selection through to the drawer.

use leptos::prelude::*;

use crate::api;
use crate::tools::ToolsRow;
use crate::types::{ModelSummary, Page};

use super::create_form::CreateForm;
use super::drawer::Drawer;
use super::table::Table;

const PAGE_LIMIT: u32 = 25;

#[component]
pub fn RecordsPane(
    target_key: ReadSignal<Option<String>>,
    target_mode: Signal<Option<String>>,
    models: ReadSignal<Vec<ModelSummary>>,
    selected_model: ReadSignal<Option<String>>,
) -> impl IntoView {
    let (page, set_page) = signal(Option::<Page>::None);
    let (load_error, set_load_error) = signal(Option::<String>::None);
    let (cursor_stack, set_cursor_stack) = signal(Vec::<Option<String>>::new());
    let (selected_row, set_selected_row) =
        signal(Option::<serde_json::Map<String, serde_json::Value>>::None);
    let (creating, set_creating) = signal(false);
    let (reload_token, set_reload_token) = signal(0u32);

    let load = move |cursor: Option<String>| {
        let Some(target) = target_key.get() else { return };
        let Some(model) = selected_model.get() else { return };
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
        if let Some(p) = page.get()
            && let Some(c) = p.next_cursor
        {
            set_cursor_stack.update(|s| s.push(Some(c.clone())));
            load(Some(c));
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

    let is_rw = Signal::derive(move || target_mode.get().as_deref() == Some("rw"));

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
        <div class="flex gap-4 h-full">
            <div class="flex-1 min-w-0 space-y-2">
                <ToolsRow target=target_signal model=model_signal pk=pk_signal />
                <ModelHeaderRow
                    current_model
                    target_mode
                    is_rw
                    creating
                    set_creating
                />
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
                } else { ().into_any() }}

                {move || match (current_model.get(), load_error.get(), page.get()) {
                    (None, _, _) => view! { <p class="text-base-content/50 text-sm">"Select a model."</p> }.into_any(),
                    (_, Some(e), _) => view! {
                        <div role="alert" class="alert alert-error">{e}</div>
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
                    _ => view! { <span class="loading loading-dots loading-sm text-base-content/40" /> }.into_any(),
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
fn ModelHeaderRow(
    current_model: Signal<Option<ModelSummary>>,
    target_mode: Signal<Option<String>>,
    is_rw: Signal<bool>,
    creating: ReadSignal<bool>,
    set_creating: WriteSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="flex items-center gap-2.5 text-sm">
            {move || current_model.get().map(|m| view! {
                <h2 class="text-base font-semibold text-base-content tracking-tight">{m.name.clone()}</h2>
            }.into_any()).unwrap_or_else(|| ().into_any())}
            {move || target_mode.get().map(|m| {
                let upper = m.to_uppercase();
                let class = if m == "rw" {
                    "badge badge-success badge-sm font-semibold"
                } else {
                    "badge badge-ghost badge-sm font-semibold"
                };
                view! { <span class=class>{upper}</span> }.into_any()
            }).unwrap_or_else(|| ().into_any())}
            <span class="flex-1" />
            {move || if is_rw.get() && !creating.get() {
                view! {
                    <button
                        class="btn btn-primary btn-sm gap-1.5"
                        on:click=move |_| set_creating.set(true)
                    >
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"
                             stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4">
                            <path d="M12 5v14M5 12h14" />
                        </svg>
                        "New"
                    </button>
                }.into_any()
            } else {
                ().into_any()
            }}
        </div>
    }
}

