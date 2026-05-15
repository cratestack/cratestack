//! `GET /api/targets/:key/schema` — `OwnedSchemaSummary` for the target.
//! `GET /api/targets/:key/models` — model summaries with field names + PK.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use cratestack_core::OwnedSchemaSummary;
use serde::Serialize;

use crate::api::ApiError;
use crate::workspace::LoadedWorkspace;

pub async fn target_schema(
    State(state): State<Arc<LoadedWorkspace>>,
    Path(key): Path<String>,
) -> Result<Json<OwnedSchemaSummary>, ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;
    Ok(Json(target.schema.summary()))
}

#[derive(Debug, Serialize)]
pub struct ModelSummary {
    pub name: String,
    pub primary_key: Option<String>,
    pub fields: Vec<FieldSummary>,
}

#[derive(Debug, Serialize)]
pub struct FieldSummary {
    pub name: String,
    pub type_name: String,
    pub arity: &'static str,
    pub is_id: bool,
    pub is_relation: bool,
}

#[derive(Debug, Serialize)]
pub struct ModelListResponse {
    pub models: Vec<ModelSummary>,
}

pub async fn list_models(
    State(state): State<Arc<LoadedWorkspace>>,
    Path(key): Path<String>,
) -> Result<Json<ModelListResponse>, ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;
    let schema = &target.schema;
    let model_names: std::collections::HashSet<&str> =
        schema.models.iter().map(|m| m.name.as_str()).collect();

    let models = schema
        .models
        .iter()
        .map(|m| {
            let primary_key = m
                .fields
                .iter()
                .find(|f| f.attributes.iter().any(|a| a.raw.starts_with("@id")))
                .map(|f| f.name.clone());
            let fields = m
                .fields
                .iter()
                .map(|f| FieldSummary {
                    name: f.name.clone(),
                    type_name: f.ty.name.clone(),
                    arity: arity_to_str(f.ty.arity),
                    is_id: f.attributes.iter().any(|a| a.raw.starts_with("@id")),
                    is_relation: model_names.contains(f.ty.name.as_str()),
                })
                .collect();
            ModelSummary {
                name: m.name.clone(),
                primary_key,
                fields,
            }
        })
        .collect();

    Ok(Json(ModelListResponse { models }))
}

fn arity_to_str(arity: cratestack_core::TypeArity) -> &'static str {
    match arity {
        cratestack_core::TypeArity::Required => "required",
        cratestack_core::TypeArity::Optional => "optional",
        cratestack_core::TypeArity::List => "list",
    }
}
