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
            <input
                type="search"
                class="border border-slate-300 rounded px-2 py-1 text-sm w-56 bg-white"
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
                    <ul class="absolute right-0 mt-1 w-80 max-h-80 overflow-auto bg-white border border-slate-200 rounded shadow-lg text-sm z-10">
                        {hits.get().into_iter().take(30).map(|h| {
                            let header = format!("{} · {}", h.kind, h.model.clone().unwrap_or_default());
                            view! {
                                <li class="px-3 py-1.5 border-b border-slate-100 hover:bg-slate-50">
                                    <div class="text-xs text-slate-500">{header}</div>
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
