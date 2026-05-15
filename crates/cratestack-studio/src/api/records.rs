//! Record endpoints: list, get, follow-relation, and the
//! write triplet (create / update / delete in [`writes`]).
//!
//! Each handler resolves the target via [`resolve_target`], then
//! delegates to the [`crate::data::DataSource`] hanging off it.

mod writes;

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use cratestack_core::Model;
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::config::TargetMode;
use crate::data::model_info::resolve_model;
use crate::data::relations::{extract_filter_value, resolve_relation};
use crate::data::{Page, PageRequest, Row};
use crate::workspace::{LoadedTarget, LoadedWorkspace};

pub use writes::{create_record, delete_record, update_record};

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct RecordResponse {
    pub row: Row,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum FollowResponse {
    Single { row: Option<Row> },
    Page(Page),
}

pub async fn list_records(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model)): Path<(String, String)>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page>, ApiError> {
    let target = resolve_target(&state, &key)?;
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

pub async fn get_record(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model, pk)): Path<(String, String, String)>,
) -> Result<Json<RecordResponse>, ApiError> {
    let target = resolve_target(&state, &key)?;
    let row = target
        .source
        .get(&model, &pk)
        .await?
        .ok_or_else(|| ApiError::InvalidPrimaryKey(pk.clone(), "no row with this id".to_owned()))?;
    Ok(Json(RecordResponse { row }))
}

/// `GET /api/targets/:key/models/:model/records/:pk/rel/:field`.
pub async fn follow_relation(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model, pk, field)): Path<(String, String, String, String)>,
    Query(query): Query<ListQuery>,
) -> Result<Json<FollowResponse>, ApiError> {
    let target = resolve_target(&state, &key)?;
    let (source_model, source_info) = resolve_model(&target.schema, &model)?;
    let relation = resolve_relation(&target.schema, source_model, &field)?;

    let source_row = target
        .source
        .get(&model, &pk)
        .await?
        .ok_or_else(|| ApiError::InvalidPrimaryKey(pk.clone(), "no row with this id".to_owned()))?;
    let filter_value = extract_filter_value(&source_row, &source_info, &relation)?;

    let body = if relation.single {
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
        FollowResponse::Single {
            row: page.rows.pop(),
        }
    } else {
        FollowResponse::Page(
            target
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
                .await?,
        )
    };
    Ok(Json(body))
}

/// Look up a target on the workspace facade, mapping the absence
/// into a structured `UNKNOWN_TARGET` API error. Used by every handler.
pub(super) fn resolve_target<'a>(
    state: &'a LoadedWorkspace,
    key: &str,
) -> Result<&'a Arc<LoadedTarget>, ApiError> {
    state
        .target(key)
        .ok_or_else(|| ApiError::UnknownTarget(key.to_owned()))
}

/// Find a model on the target's schema by name, mapping the absence
/// into `UNKNOWN_MODEL`.
pub(super) fn target_model<'a>(
    target: &'a LoadedTarget,
    model: &str,
) -> Result<&'a Model, ApiError> {
    target
        .schema
        .models
        .iter()
        .find(|m| m.name == model)
        .ok_or_else(|| ApiError::UnknownModel(model.to_owned()))
}

/// Reject mutation requests against read-only targets at the earliest
/// point — before we touch the data source.
pub(super) fn require_writable(target: &LoadedTarget) -> Result<(), ApiError> {
    if matches!(target.mode, TargetMode::Rw) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

pub(super) fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
