//! `GET /api/targets/:key/search?q=…` — case-insensitive substring
//! search over the target's parsed schema. Returns hits across models,
//! fields, types, enums (and variants), mixins, and procedures.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::search::{SearchHit, search};
use crate::workspace::LoadedWorkspace;

#[derive(Debug, Deserialize, Default)]
pub struct SearchQuery {
    pub q: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
}

pub async fn schema_search(
    State(state): State<Arc<LoadedWorkspace>>,
    Path(key): Path<String>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;
    let query = q.q.unwrap_or_default();
    let hits = search(&target.schema, &query);
    Ok(Json(SearchResponse { hits }))
}
