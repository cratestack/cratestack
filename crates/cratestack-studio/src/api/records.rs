//! Paginated record list + single-record-by-PK + relation-follow endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::data::model_info::resolve_model;
use crate::data::relations::{extract_filter_value, resolve_relation};
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

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum FollowResponse {
    Single { row: Option<Row> },
    Page(Page),
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
