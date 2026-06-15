//! Top-level Studio UI.
//!
//! The root [`App`] component owns workspace + selected-target state.
//! Every other panel (header, sidebar, records pane, drawer) is a
//! component in a sibling submodule so each file stays focused.

mod actions_row;
mod create_form;
mod drawer;
mod fields;
mod format;
mod header;
mod records_pane;
mod relations;
mod sidebar;
mod table;
mod target_switcher;
mod workspace;

use leptos::prelude::*;

use crate::api;
use crate::types::TargetSummary;

use header::Header;
use workspace::Workspace;

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
        <div class="min-h-screen flex flex-col bg-base-200">
            <Header
                workspace_name
                target_list=targets
                selected=selected_target
                set_selected=set_selected_target
            />
            <main class="flex-1 flex min-h-0">
                {move || match boot_error.get() {
                    Some(e) => view! {
                        <div role="alert" class="alert alert-error m-8 max-w-xl">
                            <span>
                                <strong class="block">"Failed to load workspace"</strong>
                                {e}
                            </span>
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
