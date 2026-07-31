//! `GET /api/targets/:key/models/:model/sql?op=…&pk=…&explain=…` —
//! render the SQL Studio would run for an operation. Useful for
//! understanding what the abstraction is doing and for copy-pasting
//! into a query tool.
//!
//! `explain=true` additionally asks the driver to *plan* that SQL and
//! returns the plan in `plan`. That is the one part of this endpoint
//! that reaches the database; it never executes the statement (Studio
//! has no `EXPLAIN ANALYZE` path) and refuses mutations outright — see
//! [`crate::data::EXPLAIN_READ_ONLY_NOTE`].

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;

use crate::api::ApiError;
use crate::data::{DataError, SqlOp, SqlPreview};
use crate::workspace::LoadedWorkspace;

#[derive(Debug, Deserialize, Default)]
pub struct PreviewQuery {
    pub op: Option<SqlOp>,
    pub pk: Option<String>,
    /// Off unless asked for: rendering SQL is pure, planning it is a
    /// round trip to the database, and the caller should opt into that
    /// cost rather than pay it on every "Show SQL" click.
    #[serde(default)]
    pub explain: Option<bool>,
}

pub async fn preview_sql(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model)): Path<(String, String)>,
    Query(q): Query<PreviewQuery>,
) -> Result<Json<SqlPreview>, ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;
    let op = q.op.unwrap_or(SqlOp::List);
    let mut preview = target
        .source
        .preview_sql(op, &model, q.pk.as_deref(), None)
        .await?;

    if q.explain.unwrap_or(false) {
        match target.source.explain(op, &model, q.pk.as_deref()).await {
            Ok(plan) => {
                preview.plan = plan.text;
                // A note from the planner explains an *absent* plan, so
                // it must not clobber a driver caveat the preview
                // already carries.
                if preview.notes.is_none() {
                    preview.notes = plan.note;
                }
            }
            // A backend that can't plan at all shouldn't cost the
            // caller the SQL preview they already successfully got.
            Err(DataError::Unsupported { what }) => {
                preview.notes.get_or_insert_with(|| what.to_owned());
            }
            Err(e) => return Err(e.into()),
        }
    }

    Ok(Json(preview))
}
