//! Right-side drawer showing the selected row and its actions.

use leptos::prelude::*;

use crate::api;
use crate::editors::build_payload;
use crate::types::{FieldError, FieldSummary, ModelSummary};

use super::actions_row::{ActionsRow, EditValues};
use super::fields::{EditFields, FieldList, row_pk, snapshot_for_edit};
use super::relations::{FollowPanel, FollowResult, RelationPicker};

type Row = serde_json::Map<String, serde_json::Value>;

#[component]
pub fn Drawer(
    target_key: ReadSignal<Option<String>>,
    target_mode: Signal<Option<String>>,
    current_model: Signal<Option<ModelSummary>>,
    selected_row: ReadSignal<Option<Row>>,
    set_selected_row: WriteSignal<Option<Row>>,
    on_changed: Callback<()>,
) -> impl IntoView {
    let (snippet, set_snippet) = signal(Option::<String>::None);
    let (snippet_status, set_snippet_status) = signal(String::new());
    let (follow_panel, set_follow_panel) = signal(Option::<FollowResult>::None);
    let (relation_field, set_relation_field) = signal(String::new());
    let (edit_values, set_edit_values): (ReadSignal<EditValues>, WriteSignal<EditValues>) =
        signal(None);
    let (edit_errors, set_edit_errors) = signal(Vec::<FieldError>::new());
    let (action_status, set_action_status) = signal(Option::<String>::None);

    let pk_value = Signal::derive(move || selected_row.get().as_ref().and_then(row_pk));
    let is_rw = Signal::derive(move || target_mode.get().as_deref() == Some("rw"));

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
        set_edit_values.set(Some(snapshot_for_edit(&row, &model)));
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
        <aside class="w-96 shrink-0 border-l border-slate-200 bg-white p-5 overflow-auto">
            <h3 class="text-[11px] font-semibold uppercase tracking-wider text-slate-400 mb-3">"Record"</h3>
            {move || match selected_row.get() {
                None => view! { <p class="text-sm text-slate-400">"Select a row to inspect it."</p> }.into_any(),
                Some(row) => view! {
                    <div class="space-y-4">
                        {move || match edit_values.get() {
                            None => view! { <FieldList row=row.clone() /> }.into_any(),
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
                        <ActionsRow
                            is_rw editing=edit_values
                            start_edit=Callback::new(start_edit)
                            cancel_edit=Callback::new(cancel_edit)
                            save_edit=Callback::new(save_edit)
                            delete_row=Callback::new(delete_row)
                            action_status
                            copy_snippet=Callback::new(copy_snippet)
                            snippet snippet_status
                        />
                    </div>
                }.into_any(),
            }}
        </aside>
    }
}

