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
        return view! { <span class="text-base-content/30">"—"</span> }.into_any();
    }
    let text = value.map(format_cell).unwrap_or_default();
    match value {
        Some(serde_json::Value::Bool(b)) => {
            let class = if *b { "badge badge-success badge-sm" } else { "badge badge-ghost badge-sm" };
            view! { <span class=class>{text}</span> }.into_any()
        }
        Some(serde_json::Value::Number(_)) => {
            view! { <span class="font-mono text-sm tabular-nums text-base-content/80">{text}</span> }.into_any()
        }
        _ if field.is_enum => {
            view! { <span class="badge badge-ghost badge-sm">{text}</span> }.into_any()
        }
        _ if field.is_id => {
            view! { <span class="font-mono text-xs text-base-content/60">{text}</span> }.into_any()
        }
        _ => view! { <span class="text-base-content/80">{text}</span> }.into_any(),
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

    view! {
        <div class="space-y-3">
            <div class="overflow-x-auto border border-base-300 rounded-box bg-base-100 shadow-sm">
                <table class="table table-xs">
                    <thead class="text-base-content/50 text-xs uppercase tracking-wider">
                        <tr>
                            {column_headers.iter().map(|c| {
                                view! { <th class="font-semibold whitespace-nowrap">{c.name.clone()}</th> }
                            }).collect_view()}
                        </tr>
                    </thead>
                    <tbody>
                        {rows.into_iter().map(|row| {
                            let row_for_handler = row.clone();
                            let cells = column_rows.iter().map(|c| {
                                let cell = render_cell(c, row.get(&c.name));
                                view! { <td class="align-middle whitespace-nowrap">{cell}</td> }
                            }).collect_view();
                            view! {
                                <tr
                                    class="cursor-pointer hover"
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
                    class="btn btn-sm"
                    on:click=move |_| on_prev.run(())
                    disabled=move || !can_prev.get()
                >
                    "← Previous"
                </button>
                <button
                    class="btn btn-sm"
                    on:click=move |_| on_next.run(())
                    disabled=next_disabled
                >
                    "Next →"
                </button>
                <span class="text-xs text-base-content/40 ml-1">{row_count}" rows on this page"</span>
            </div>
        </div>
    }
}
