//! Audit button + dropdown showing recent writes captured by the
//! Studio backend.

use leptos::prelude::*;

use crate::api;
use crate::types::AuditEntry;

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
        <div class="dropdown dropdown-end" class:dropdown-open=move || open.get()>
            <div tabindex="0" role="button"
                class="btn btn-sm gap-1.5"
                on:click=move |_| {
                    let was = open.get();
                    set_open.set(!was);
                    if !was {
                        load();
                    }
                }
            >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                     stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 opacity-60">
                    <path d="M12 8v4l3 2" />
                    <circle cx="12" cy="12" r="9" />
                </svg>
                "Audit"
            </div>
            {move || if open.get() {
                view! {
                    <div class="fixed inset-0 z-20" on:click=move |_| set_open.set(false) />
                    <div tabindex="0" class="dropdown-content card card-compact bg-base-100 shadow-xl w-[28rem] mt-2 z-30">
                        <div class="card-body">
                            <div class="flex items-center justify-between">
                                <span class="card-title text-sm">"Recent writes"</span>
                                {move || if loading.get() {
                                    view! { <span class="loading loading-spinner loading-xs opacity-40" /> }.into_any()
                                } else {
                                    ().into_any()
                                }}
                            </div>
                            {move || if entries.get().is_empty() {
                                view! { <p class="text-xs opacity-50">"No writes captured yet."</p> }.into_any()
                            } else {
                                view! {
                                    <ul class="text-xs max-h-80 overflow-auto divide-y divide-base-200">
                                        {entries.get().into_iter().map(|e| {
                                            let pk = e.pk.unwrap_or_else(|| "·".to_owned());
                                            view! {
                                                <li class="py-1.5 grid grid-cols-[6.5rem_3.5rem_1fr] gap-2">
                                                    <span class="opacity-50 font-mono">{e.at.clone()}</span>
                                                    <span class="font-semibold">{e.op.clone()}</span>
                                                    <span class="font-mono break-all">{format!("{}/{} → {}", e.target, e.model, pk)}</span>
                                                </li>
                                            }
                                        }).collect_view()}
                                    </ul>
                                }.into_any()
                            }}
                        </div>
                    </div>
                }.into_any()
            } else {
                ().into_any()
            }}
        </div>
    }
}
