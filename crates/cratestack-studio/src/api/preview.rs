//! `GET /api/targets/:key/models/:model/sql?op=…&pk=…` — render the
//! SQL Studio would run for an operation without touching the
//! database. Useful for understanding what the abstraction is doing
//! and for copy-pasting into a query tool.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;

use crate::api::ApiError;
use crate::data::{SqlOp, SqlPreview};
use crate::workspace::LoadedWorkspace;

#[derive(Debug, Deserialize, Default)]
pub struct PreviewQuery {
    pub op: Option<SqlOp>,
    pub pk: Option<String>,
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
    let preview = target
        .source
        .preview_sql(op, &model, q.pk.as_deref(), None)
        .await?;
    Ok(Json(preview))
}
