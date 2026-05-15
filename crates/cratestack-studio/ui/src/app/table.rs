//! Records table — one row per record, click to select. Page-navigation
//! buttons live below.

use leptos::prelude::*;

use crate::types::{FieldSummary, ModelSummary, Page};

use super::format::format_cell;

#[component]
pub fn Table(
    model: ModelSummary,
    page: Page,
    on_select: WriteSignal<Option<serde_json::Map<String, serde_json::Value>>>,
    on_next: Callback<()>,
    on_prev: Callback<()>,
    can_prev: Signal<bool>,
) -> impl IntoView {
    let columns: Vec<FieldSummary> = model
        .fields
        .iter()
        .filter(|f| !f.is_relation && f.arity != "list")
        .cloned()
        .collect();
    let column_headers = columns.clone();
    let column_rows = columns;
    let next_disabled = page.next_cursor.is_none();
    let rows = page.rows;
    let row_count = rows.len();

    view! {
        <div class="space-y-3">
            <div class="text-xs text-slate-500">{row_count}" rows on this page"</div>
            <div class="overflow-x-auto border border-slate-200 rounded bg-white">
                <table class="min-w-full text-sm">
                    <thead class="bg-slate-100 text-slate-700 text-xs uppercase tracking-wide">
                        <tr>
                            {column_headers.iter().map(|c| {
                                view! { <th class="px-3 py-2 text-left font-medium">{c.name.clone()}</th> }
                            }).collect_view()}
                        </tr>
                    </thead>
                    <tbody>
                        {rows.into_iter().map(|row| {
                            let row_for_handler = row.clone();
                            let cells = column_rows.iter().map(|c| {
                                let value = row.get(&c.name).map(format_cell).unwrap_or_else(|| "—".to_owned());
                                view! { <td class="px-3 py-2 border-t border-slate-100 align-top font-mono text-xs">{value}</td> }
                            }).collect_view();
                            view! {
                                <tr
                                    class="cursor-pointer hover:bg-slate-50"
                                    on:click=move |_| on_select.set(Some(row_for_handler.clone()))
                                >
                                    {cells}
                                </tr>
                            }
                        }).collect_view()}
                    </tbody>
                </table>
            </div>
            <div class="flex items-center gap-2">
                <button
                    class="px-3 py-1 text-sm rounded border border-slate-300 bg-white disabled:opacity-40"
                    on:click=move |_| on_prev.run(())
                    disabled=move || !can_prev.get()
                >
                    "← Previous"
                </button>
                <button
                    class="px-3 py-1 text-sm rounded border border-slate-300 bg-white disabled:opacity-40"
                    on:click=move |_| on_next.run(())
                    disabled=next_disabled
                >
                    "Next →"
                </button>
            </div>
        </div>
    }
}
