//! Records table — one row per record, click to select. Page-navigation
//! buttons live below. Cells render type-aware: ids and numbers are
//! monospaced, booleans and enums become small pills.

use leptos::prelude::*;

use crate::types::{FieldSummary, ModelSummary, Page};

use super::format::format_cell;

/// Render a single cell's value according to its field type so the
/// table reads as data, not a wall of monospace.
fn render_cell(field: &FieldSummary, value: Option<&serde_json::Value>) -> impl IntoView + use<> {
    let is_null = matches!(value, None | Some(serde_json::Value::Null));
    if is_null {
        return view! { <span class="text-slate-300">"—"</span> }.into_any();
    }
    let text = value.map(format_cell).unwrap_or_default();
    match value {
        Some(serde_json::Value::Bool(b)) => {
            let class = if *b {
                "inline-flex items-center text-[11px] font-medium px-1.5 py-0.5 rounded bg-emerald-50 text-emerald-700 ring-1 ring-emerald-200"
            } else {
                "inline-flex items-center text-[11px] font-medium px-1.5 py-0.5 rounded bg-slate-100 text-slate-500 ring-1 ring-slate-200"
            };
            view! { <span class=class>{text}</span> }.into_any()
        }
        Some(serde_json::Value::Number(_)) => {
            view! { <span class="font-mono text-[13px] tabular-nums text-slate-700">{text}</span> }.into_any()
        }
        _ if field.is_enum => {
            view! {
                <span class="inline-flex items-center text-[11px] font-medium px-1.5 py-0.5 rounded bg-slate-100 text-slate-600 ring-1 ring-slate-200">
                    {text}
                </span>
            }.into_any()
        }
        _ if field.is_id => {
            view! { <span class="font-mono text-[12px] text-slate-500">{text}</span> }.into_any()
        }
        _ => view! { <span class="text-slate-700">{text}</span> }.into_any(),
    }
}

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

    let nav_btn = "inline-flex items-center gap-1 px-3 py-1.5 text-sm font-medium rounded-lg border \
                   border-slate-200 bg-white text-slate-700 shadow-sm hover:bg-slate-50 \
                   disabled:opacity-40 disabled:cursor-not-allowed transition-colors";

    view! {
        <div class="space-y-3">
            <div class="overflow-x-auto border border-slate-200 rounded-xl bg-white shadow-sm">
                <table class="min-w-full text-sm">
                    <thead class="bg-slate-50 text-slate-400 text-[11px] uppercase tracking-wider border-b border-slate-200">
                        <tr>
                            {column_headers.iter().map(|c| {
                                view! { <th class="px-4 py-2.5 text-left font-semibold whitespace-nowrap">{c.name.clone()}</th> }
                            }).collect_view()}
                        </tr>
                    </thead>
                    <tbody class="divide-y divide-slate-100">
                        {rows.into_iter().map(|row| {
                            let row_for_handler = row.clone();
                            let cells = column_rows.iter().map(|c| {
                                let cell = render_cell(c, row.get(&c.name));
                                view! { <td class="px-4 py-2.5 align-middle whitespace-nowrap">{cell}</td> }
                            }).collect_view();
                            view! {
                                <tr
                                    class="cursor-pointer hover:bg-indigo-50/40 transition-colors"
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
                    class=nav_btn
                    on:click=move |_| on_prev.run(())
                    disabled=move || !can_prev.get()
                >
                    "← Previous"
                </button>
                <button
                    class=nav_btn
                    on:click=move |_| on_next.run(())
                    disabled=next_disabled
                >
                    "Next →"
                </button>
                <span class="text-xs text-slate-400 ml-1">{row_count}" rows on this page"</span>
            </div>
        </div>
    }
}
