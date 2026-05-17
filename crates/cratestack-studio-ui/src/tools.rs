//! Phase 4 power-user surfaces: SQL preview ([`sql_preview`]),
//! schema search ([`search`]), audit log ([`audit`]), and per-model
//! drift indicators in the sidebar.

mod audit;
mod search;
mod sql_preview;

use leptos::prelude::*;

use crate::types::ModelDrift;

pub use audit::AuditButton;
pub use search::SearchBar;
pub use sql_preview::ToolsRow;

/// Per-model drift indicator rendered in the sidebar.
pub fn render_drift_dot(status: Option<&str>) -> impl IntoView + use<> {
    let (label, class) = match status {
        Some("drift") => (
            "⚠ drift",
            "ml-2 text-[10px] text-amber-800 bg-amber-100 px-1 rounded",
        ),
        Some("missing_table") => (
            "✕ table",
            "ml-2 text-[10px] text-red-800 bg-red-100 px-1 rounded",
        ),
        Some("unsupported") => ("·", "ml-2 text-[10px] text-slate-400"),
        Some("skipped") => ("?", "ml-2 text-[10px] text-slate-400"),
        Some("ok") => ("", "hidden"),
        _ => ("", "hidden"),
    };
    if label.is_empty() {
        return view! { <span></span> }.into_any();
    }
    view! { <span class=class>{label}</span> }.into_any()
}

/// Pull drift status by model name from a cached `Vec<ModelDrift>`
/// snapshot.
pub fn drift_status<'a>(snapshot: &'a [ModelDrift], model: &str) -> Option<&'a str> {
    snapshot
        .iter()
        .find(|d| d.model == model)
        .map(|d| d.status.as_str())
}
