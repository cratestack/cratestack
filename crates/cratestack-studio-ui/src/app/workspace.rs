//! Workspace pane: hosts the sidebar + the records pane for the
//! currently-selected target.

use leptos::prelude::*;

use crate::api;
use crate::types::{ModelDrift, ModelSummary};

use super::records_pane::RecordsPane;
use super::sidebar::Sidebar;

#[component]
pub fn Workspace(
    target_key: ReadSignal<Option<String>>,
    target_mode: Signal<Option<String>>,
) -> impl IntoView {
    let (models, set_models) = signal(Vec::<ModelSummary>::new());
    let (selected_model, set_selected_model) = signal(Option::<String>::None);
    let (load_error, set_load_error) = signal(Option::<String>::None);
    let (drift, set_drift) = signal(Vec::<ModelDrift>::new());

    Effect::new(move |_| {
        let Some(key) = target_key.get() else { return };
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
        <div class="flex-1 flex min-h-0">
            <Sidebar
                models
                selected=selected_model
                set_selected=set_selected_model
                drift=drift
            />
            <section class="flex-1 p-4 overflow-auto">
                {move || match load_error.get() {
                    Some(e) => view! {
                        <div role="alert" class="alert alert-error">{e}</div>
                    }.into_any(),
                    None => view! {
                        <RecordsPane target_key target_mode models selected_model />
                    }.into_any(),
                }}
            </section>
        </div>
    }
}
