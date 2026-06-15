//! The multi-cstack target switcher. A workspace can wire several
//! `.cstack` schemas (each a "target"); this is the control that picks
//! which one the rest of the UI is bound to. Replaces the bare native
//! `<select>` with a DaisyUI `menu` popover that surfaces each target's
//! display name, key, deployment mode, and data source.

use leptos::prelude::*;

use crate::types::TargetSummary;

/// DaisyUI badge for a target's mode (`rw` / `ro`).
fn mode_badge(mode: &str) -> impl IntoView + use<> {
    let class = if mode == "rw" {
        "badge badge-success badge-sm font-semibold"
    } else {
        "badge badge-ghost badge-sm font-semibold"
    };
    let label = if mode == "rw" { "RW" } else { "RO" };
    view! { <span class=class>{label}</span> }
}

/// Source glyph: a DB cylinder for db-backed targets, a cloud for API targets.
fn source_icon(has_db: bool) -> impl IntoView + use<> {
    if has_db {
        view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
                 stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 opacity-60">
                <ellipse cx="12" cy="5" rx="8" ry="3" />
                <path d="M4 5v14c0 1.66 3.58 3 8 3s8-1.34 8-3V5" />
                <path d="M4 12c0 1.66 3.58 3 8 3s8-1.34 8-3" />
            </svg>
        }
        .into_any()
    } else {
        view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
                 stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 opacity-60">
                <path d="M17.5 19a4.5 4.5 0 0 0 .5-8.97 6 6 0 0 0-11.64-1.5A4 4 0 0 0 6 19h11.5Z" />
            </svg>
        }
        .into_any()
    }
}

#[component]
pub fn TargetSwitcher(
    target_list: ReadSignal<Vec<TargetSummary>>,
    selected: ReadSignal<Option<String>>,
    set_selected: WriteSignal<Option<String>>,
) -> impl IntoView {
    let (open, set_open) = signal(false);

    let current = Signal::derive(move || {
        let key = selected.get()?;
        target_list.get().into_iter().find(|t| t.key == key)
    });

    view! {
        <div class="dropdown" class:dropdown-open=move || open.get()>
            <div tabindex="0" role="button" class="btn btn-ghost btn-sm gap-2"
                on:click=move |_| set_open.update(|o| *o = !*o)>
                {move || match current.get() {
                    Some(t) => view! {
                        {source_icon(t.has_db)}
                        <span class="text-xs uppercase tracking-wide opacity-50">"Target"</span>
                        <span class="font-semibold">{t.display_name.clone()}</span>
                        {mode_badge(&t.mode)}
                    }.into_any(),
                    None => view! { <span class="opacity-50">"No target"</span> }.into_any(),
                }}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                     stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 opacity-50">
                    <path d="m6 9 6 6 6-6" />
                </svg>
            </div>

            {move || if open.get() {
                view! {
                    <div class="fixed inset-0 z-20" on:click=move |_| set_open.set(false) />
                    <ul tabindex="0" class="dropdown-content menu bg-base-100 rounded-box shadow-xl w-80 mt-2 z-30">
                        <li class="menu-title">"Schemas in this workspace"</li>
                        {move || target_list.get().into_iter().map(|t| {
                            let key = t.key.clone();
                            let key_click = key.clone();
                            let is_active = selected.get().as_deref() == Some(key.as_str());
                            view! {
                                <li>
                                    <a
                                        class="gap-3"
                                        class:active=is_active
                                        on:click=move |_| {
                                            set_selected.set(Some(key_click.clone()));
                                            set_open.set(false);
                                        }
                                    >
                                        {source_icon(t.has_db)}
                                        <span class="flex flex-col min-w-0 flex-1">
                                            <span class="font-medium truncate">{t.display_name.clone()}</span>
                                            <span class="font-mono text-xs opacity-50 truncate">
                                                {format!("{} · {}", key, if t.has_db { "database" } else { "api" })}
                                            </span>
                                        </span>
                                        {mode_badge(&t.mode)}
                                    </a>
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
