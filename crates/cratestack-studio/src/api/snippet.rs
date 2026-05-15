//! `GET /api/targets/:key/models/:model/snippet?pk=…` — render a Rust
//! find_unique snippet for the (model, pk) pair.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::snippet::rust_find_unique;
use crate::workspace::LoadedWorkspace;

#[derive(Debug, Deserialize)]
pub struct SnippetQuery {
    pub pk: String,
}

#[derive(Debug, Serialize)]
pub struct SnippetResponse {
    pub rust: String,
}

pub async fn record_snippet(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model)): Path<(String, String)>,
    Query(query): Query<SnippetQuery>,
) -> Result<Json<SnippetResponse>, ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;
    let rust = rust_find_unique(&target.schema, &model, &query.pk)?;
    Ok(Json(SnippetResponse { rust }))
}
