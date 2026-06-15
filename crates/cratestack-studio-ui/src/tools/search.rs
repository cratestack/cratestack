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
        <div class="dropdown dropdown-end" class:dropdown-open=move || open.get() && !hits.get().is_empty()>
            <label class="input input-bordered input-sm flex items-center gap-2 w-60">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                     stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4 opacity-40">
                    <circle cx="11" cy="11" r="7" />
                    <path d="m21 21-4.3-4.3" />
                </svg>
                <input
                    type="search"
                    class="grow"
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
            </label>
            {move || if open.get() && !hits.get().is_empty() {
                view! {
                    <ul class="dropdown-content menu bg-base-100 rounded-box shadow-xl w-80 max-h-80 flex-nowrap overflow-auto mt-2 z-30">
                        {hits.get().into_iter().take(30).map(|h| {
                            let header = format!("{} · {}", h.kind, h.model.clone().unwrap_or_default());
                            view! {
                                <li>
                                    <a class="flex flex-col items-start gap-0">
                                        <span class="text-xs uppercase tracking-wide opacity-40">{header}</span>
                                        <span class="font-medium">{h.name.clone()}</span>
                                        <span class="text-xs opacity-60">{h.detail.clone()}</span>
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
