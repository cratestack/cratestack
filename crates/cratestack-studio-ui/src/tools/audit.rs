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
        <button
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-slate-200 bg-white text-sm \
                   font-medium text-slate-700 shadow-sm hover:bg-slate-50 hover:border-slate-300 transition-colors"
            on:click=move |_| {
                let was = open.get();
                set_open.set(!was);
                if !was {
                    load();
                }
            }
        >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                 stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 text-slate-400">
                <path d="M12 8v4l3 2" />
                <circle cx="12" cy="12" r="9" />
            </svg>
            "Audit"
        </button>
        {move || if open.get() {
            view! {
                <div class="absolute right-4 top-14 w-[28rem] max-h-[28rem] overflow-auto bg-white border border-slate-200 rounded-xl shadow-xl z-30">
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
