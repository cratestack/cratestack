//! Write-side record handlers — POST / PATCH / DELETE. Each handler
//! resolves the target, enforces RW mode, validates the payload
//! against the schema model, and delegates to the data source.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::api::ApiError;
use crate::audit::AuditOp;
use crate::validators::validate_payload;
use crate::workspace::LoadedWorkspace;

use super::{
    RecordResponse, require_writable, resolve_target, target_model, value_to_string,
};

/// `POST /api/targets/:key/models/:model/records`
pub async fn create_record(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model)): Path<(String, String)>,
    Json(payload): Json<serde_json::Map<String, serde_json::Value>>,
) -> Result<(StatusCode, Json<RecordResponse>), ApiError> {
    let target = resolve_target(&state, &key)?;
    require_writable(target)?;

    let model_decl = target_model(target, &model)?;
    let errors = validate_payload(model_decl, &payload, false);
    if !errors.is_empty() {
        return Err(ApiError::Validation(errors));
    }

    let row = target.source.create(&model, &payload).await?;
    let pk_field = model_decl
        .fields
        .iter()
        .find(|f| f.attributes.iter().any(|a| a.raw.starts_with("@id")))
        .map(|f| f.name.as_str())
        .unwrap_or("id");
    let pk_value = row.get(pk_field).map(value_to_string);
    state.audit.push(&target.key, &model, AuditOp::Create, pk_value);
    Ok((StatusCode::CREATED, Json(RecordResponse { row })))
}

/// `PATCH /api/targets/:key/models/:model/records/:pk`
pub async fn update_record(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model, pk)): Path<(String, String, String)>,
    Json(payload): Json<serde_json::Map<String, serde_json::Value>>,
) -> Result<Json<RecordResponse>, ApiError> {
    let target = resolve_target(&state, &key)?;
    require_writable(target)?;

    let model_decl = target_model(target, &model)?;
    let errors = validate_payload(model_decl, &payload, true);
    if !errors.is_empty() {
        return Err(ApiError::Validation(errors));
    }

    let row = target
        .source
        .update(&model, &pk, &payload)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidPrimaryKey(pk.clone(), "no row with this id".to_owned())
        })?;
    state
        .audit
        .push(&target.key, &model, AuditOp::Update, Some(pk.clone()));
    Ok(Json(RecordResponse { row }))
}

/// `DELETE /api/targets/:key/models/:model/records/:pk`
pub async fn delete_record(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model, pk)): Path<(String, String, String)>,
) -> Result<Json<RecordResponse>, ApiError> {
    let target = resolve_target(&state, &key)?;
    require_writable(target)?;

    let row = target
        .source
        .delete(&model, &pk)
        .await?
        .ok_or_else(|| {
            ApiError::InvalidPrimaryKey(pk.clone(), "no row with this id".to_owned())
        })?;
    state
        .audit
        .push(&target.key, &model, AuditOp::Delete, Some(pk.clone()));
    Ok(Json(RecordResponse { row }))
}
