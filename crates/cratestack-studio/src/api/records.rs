//! Paginated record list + single-record-by-PK + relation-follow + write endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::config::TargetMode;
use crate::data::model_info::resolve_model;
use crate::data::relations::{extract_filter_value, resolve_relation};
use crate::data::{Page, PageRequest, Row};
use crate::validators::validate_payload;
use crate::workspace::{LoadedTarget, LoadedWorkspace};

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

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum FollowResponse {
    Single { row: Option<Row> },
    Page(Page),
}

/// Reject mutation requests against read-only targets at the
/// earliest point — before we touch the data source.
fn require_writable(target: &LoadedTarget) -> Result<(), ApiError> {
    if matches!(target.mode, TargetMode::Rw) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

/// `POST /api/targets/:key/models/:model/records`
pub async fn create_record(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model)): Path<(String, String)>,
    Json(payload): Json<serde_json::Map<String, serde_json::Value>>,
) -> Result<(StatusCode, Json<RecordResponse>), ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;
    require_writable(target)?;

    let model_decl = target
        .schema
        .models
        .iter()
        .find(|m| m.name == model)
        .ok_or_else(|| ApiError::UnknownModel(model.clone()))?;
    let errors = validate_payload(model_decl, &payload, false);
    if !errors.is_empty() {
        return Err(ApiError::Validation(errors));
    }

    let row = target.source.create(&model, &payload).await?;
    Ok((StatusCode::CREATED, Json(RecordResponse { row })))
}

/// `PATCH /api/targets/:key/models/:model/records/:pk`
pub async fn update_record(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model, pk)): Path<(String, String, String)>,
    Json(payload): Json<serde_json::Map<String, serde_json::Value>>,
) -> Result<Json<RecordResponse>, ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;
    require_writable(target)?;

    let model_decl = target
        .schema
        .models
        .iter()
        .find(|m| m.name == model)
        .ok_or_else(|| ApiError::UnknownModel(model.clone()))?;
    let errors = validate_payload(model_decl, &payload, true);
    if !errors.is_empty() {
        return Err(ApiError::Validation(errors));
    }

    let row = target.source.update(&model, &pk, &payload).await?.ok_or_else(|| {
        ApiError::InvalidPrimaryKey(pk.clone(), "no row with this id".to_owned())
    })?;
    Ok(Json(RecordResponse { row }))
}

/// `DELETE /api/targets/:key/models/:model/records/:pk`
pub async fn delete_record(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model, pk)): Path<(String, String, String)>,
) -> Result<Json<RecordResponse>, ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;
    require_writable(target)?;

    let row = target
        .source
        .delete(&model, &pk)
        .await?
        .ok_or_else(|| ApiError::InvalidPrimaryKey(pk.clone(), "no row with this id".to_owned()))?;
    Ok(Json(RecordResponse { row }))
}

/// `GET /api/targets/:key/models/:model/records/:pk/rel/:field`.
///
/// Resolves the relation from the source model's schema, fetches the
/// source row to read its FK column, then delegates to the
/// [`DataSource::follow`] method. Returns a page for List-arity
/// relations and a single optional row for Required-arity relations.
pub async fn follow_relation(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model, pk, field)): Path<(String, String, String, String)>,
    Query(query): Query<ListQuery>,
) -> Result<Json<FollowResponse>, ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;

    let (source_model, source_info) = resolve_model(&target.schema, &model)?;
    let relation = resolve_relation(&target.schema, source_model, &field)?;

    let source_row = target
        .source
        .get(&model, &pk)
        .await?
        .ok_or_else(|| ApiError::InvalidPrimaryKey(pk.clone(), "no row with this id".to_owned()))?;

    let filter_value = extract_filter_value(&source_row, &source_info, &relation)?;

    if relation.single {
        let mut page = target
            .source
            .follow(
                &relation.target_model.name,
                &relation.filter_column,
                relation.filter_cast,
                &filter_value,
                PageRequest {
                    cursor: None,
                    limit: Some(1),
                },
            )
            .await?;
        Ok(Json(FollowResponse::Single {
            row: page.rows.pop(),
        }))
    } else {
        let page = target
            .source
            .follow(
                &relation.target_model.name,
                &relation.filter_column,
                relation.filter_cast,
                &filter_value,
                PageRequest {
                    cursor: query.cursor.as_deref(),
                    limit: query.limit,
                },
            )
            .await?;
        Ok(Json(FollowResponse::Page(page)))
    }
}
