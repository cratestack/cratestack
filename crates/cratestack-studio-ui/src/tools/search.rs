//! Schema-search box mounted in the header.

use leptos::prelude::*;

use crate::api;
use crate::types::SearchHit;

#[component]
pub fn SearchBar(target: Signal<Option<String>>) -> impl IntoView {
    let (query, set_query) = signal(String::new());
    let (hits, set_hits) = signal(Vec::<SearchHit>::new());
    let (open, set_open) = signal(false);

    let trigger = move || {
        let q = query.get();
        let Some(t) = target.get() else { return };
        if q.trim().is_empty() {
            set_hits.set(Vec::new());
            set_open.set(false);
            return;
        }
        leptos::task::spawn_local(async move {
            match api::schema_search(&t, &q).await {
                Ok(resp) => {
                    set_hits.set(resp.hits);
                    set_open.set(true);
                }
                Err(_) => {
                    set_hits.set(Vec::new());
                    set_open.set(false);
                }
            }
        });
    };

    view! {
        <div class="relative">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                 stroke-linecap="round" stroke-linejoin="round"
                 class="w-4 h-4 text-slate-400 absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none">
                <circle cx="11" cy="11" r="7" />
                <path d="m21 21-4.3-4.3" />
            </svg>
            <input
                type="search"
                class="border border-slate-200 rounded-lg pl-8 pr-3 py-1.5 text-sm w-60 bg-slate-50 \
                       placeholder:text-slate-400 focus:bg-white focus:border-indigo-400 \
                       focus:ring-2 focus:ring-indigo-100 focus:outline-none transition-colors"
                placeholder="Search schema…"
                prop:value=move || query.get()
                on:input=move |ev| {
                    set_query.set(event_target_value(&ev));
                    trigger();
                }
                on:focus=move |_| {
                    if !hits.get_untracked().is_empty() {
                        set_open.set(true);
                    }
                }
                on:blur=move |_| {
                    // Defer so click handlers on the dropdown still fire.
                    let s = set_open;
                    leptos::task::spawn_local(async move {
                        gloo_timers::future::TimeoutFuture::new(120).await;
                        s.set(false);
                    });
                }
            />
            {move || if open.get() && !hits.get().is_empty() {
                view! {
                    <ul class="absolute right-0 mt-2 w-80 max-h-80 overflow-auto bg-white border border-slate-200 rounded-xl shadow-xl text-sm z-30 p-1">
                        {hits.get().into_iter().take(30).map(|h| {
                            let header = format!("{} · {}", h.kind, h.model.clone().unwrap_or_default());
                            view! {
                                <li class="px-2.5 py-1.5 rounded-lg hover:bg-slate-50">
                                    <div class="text-[10px] uppercase tracking-wide text-slate-400">{header}</div>
                                    <div class="font-medium text-slate-800">{h.name.clone()}</div>
                                    <div class="text-xs text-slate-500">{h.detail.clone()}</div>
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
