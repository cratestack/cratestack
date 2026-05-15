//! Paginated record list + single-record-by-PK endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::data::{Page, PageRequest, Row};
use crate::workspace::LoadedWorkspace;

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

pub async fn list_records(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model)): Path<(String, String)>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page>, ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;
    let page = target
        .source
        .list(
            &model,
            PageRequest {
                cursor: query.cursor.as_deref(),
                limit: query.limit,
            },
        )
        .await?;
    Ok(Json(page))
}

#[derive(Debug, Serialize)]
pub struct RecordResponse {
    pub row: Row,
}

pub async fn get_record(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model, pk)): Path<(String, String, String)>,
) -> Result<Json<RecordResponse>, ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;
    let row = target
        .source
        .get(&model, &pk)
        .await?
        .ok_or_else(|| ApiError::InvalidPrimaryKey(pk.clone(), "no row with this id".to_owned()))?;
    Ok(Json(RecordResponse { row }))
}
