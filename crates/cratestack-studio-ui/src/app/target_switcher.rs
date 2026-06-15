//! The multi-cstack target switcher. A workspace can wire several
//! `.cstack` schemas (each a "target"); this is the control that picks
//! which one the rest of the UI is bound to. Replaces the bare native
//! `<select>` with a popover that surfaces each target's display name,
//! key, deployment mode, and data source.

use leptos::prelude::*;

use crate::types::TargetSummary;

/// Small slate badge for a target's mode (`rw` / `ro`).
fn mode_badge(mode: &str) -> impl IntoView + use<> {
    let (label, class) = if mode == "rw" {
        (
            "RW",
            "text-[10px] font-semibold tracking-wide px-1.5 py-0.5 rounded bg-emerald-50 text-emerald-700 ring-1 ring-emerald-200",
        )
    } else {
        (
            "RO",
            "text-[10px] font-semibold tracking-wide px-1.5 py-0.5 rounded bg-slate-100 text-slate-500 ring-1 ring-slate-200",
        )
    };
    view! { <span class=class>{label}</span> }
}

/// Source glyph: a DB cylinder for db-backed targets, a cloud for API
/// targets. Rendered inside a rounded tile so it reads as an "icon".
fn source_icon(has_db: bool) -> impl IntoView + use<> {
    let inner = if has_db {
        view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
                 stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4">
                <ellipse cx="12" cy="5" rx="8" ry="3" />
                <path d="M4 5v14c0 1.66 3.58 3 8 3s8-1.34 8-3V5" />
                <path d="M4 12c0 1.66 3.58 3 8 3s8-1.34 8-3" />
            </svg>
        }
        .into_any()
    } else {
        view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
                 stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4">
                <path d="M17.5 19a4.5 4.5 0 0 0 .5-8.97 6 6 0 0 0-11.64-1.5A4 4 0 0 0 6 19h11.5Z" />
            </svg>
        }
        .into_any()
    };
    view! {
        <span class="flex items-center justify-center w-8 h-8 rounded-lg bg-slate-100 text-slate-500 shrink-0">
            {inner}
        </span>
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
        <div class="relative">
            <button
                class="flex items-center gap-2.5 rounded-lg border border-slate-200 bg-white pl-2 pr-3 py-1.5 \
                       shadow-sm hover:border-slate-300 transition-colors min-w-[12rem]"
                on:click=move |_| set_open.update(|o| *o = !*o)
            >
                {move || match current.get() {
                    Some(t) => view! {
                        {source_icon(t.has_db)}
                        <span class="flex flex-col items-start leading-tight min-w-0">
                            <span class="text-[10px] uppercase tracking-wide text-slate-400">"Target"</span>
                            <span class="text-sm font-semibold text-slate-900 truncate max-w-[9rem]">
                                {t.display_name.clone()}
                            </span>
                        </span>
                        {mode_badge(&t.mode)}
                    }.into_any(),
                    None => view! {
                        <span class="text-sm text-slate-400 px-1">"No target"</span>
                    }.into_any(),
                }}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                     stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 text-slate-400">
                    <path d="m6 9 6 6 6-6" />
                </svg>
            </button>

            {move || if open.get() {
                view! {
                    // Click-away backdrop.
                    <div class="fixed inset-0 z-20" on:click=move |_| set_open.set(false) />
                    <div class="absolute left-0 z-30 mt-2 w-80 rounded-xl border border-slate-200 bg-white shadow-xl p-1.5">
                        <div class="px-2.5 pt-1.5 pb-1 text-[10px] uppercase tracking-wide text-slate-400">
                            "Schemas in this workspace"
                        </div>
                        <ul class="space-y-0.5">
                            {move || target_list.get().into_iter().map(|t| {
                                let key = t.key.clone();
                                let key_click = key.clone();
                                let is_active = selected.get().as_deref() == Some(key.as_str());
                                let row_class = if is_active {
                                    "w-full flex items-center gap-3 rounded-lg px-2 py-2 text-left bg-indigo-50"
                                } else {
                                    "w-full flex items-center gap-3 rounded-lg px-2 py-2 text-left hover:bg-slate-50"
                                };
                                view! {
                                    <li>
                                        <button
                                            class=row_class
                                            on:click=move |_| {
                                                set_selected.set(Some(key_click.clone()));
                                                set_open.set(false);
                                            }
                                        >
                                            {source_icon(t.has_db)}
                                            <span class="flex flex-col min-w-0 flex-1">
                                                <span class="text-sm font-medium text-slate-900 truncate">
                                                    {t.display_name.clone()}
                                                </span>
                                                <span class="font-mono text-[11px] text-slate-400 truncate">
                                                    {format!("{} · {}", key, if t.has_db { "database" } else { "api" })}
                                                </span>
                                            </span>
                                            {mode_badge(&t.mode)}
                                            {if is_active {
                                                view! {
                                                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor"
                                                         stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"
                                                         class="w-4 h-4 text-indigo-600 shrink-0">
                                                        <path d="M20 6 9 17l-5-5" />
                                                    </svg>
                                                }.into_any()
                                            } else {
                                                view! { <span class="w-4 shrink-0" /> }.into_any()
                                            }}
                                        </button>
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    </div>
                }.into_any()
            } else {
                ().into_any()
            }}
        </div>
    }
}
