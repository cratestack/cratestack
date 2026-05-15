//! `GET /api/targets/:key/drift` — compare the `.cstack` schema's
//! declared columns against the live database. The UI uses this to
//! flag tables that need a migration before a write would succeed.
//!
//! Each model is checked against the underlying driver's catalog
//! (`information_schema` on Postgres, `PRAGMA table_info` on SQLite).
//! API-only targets are reported as "unsupported" since Studio can't
//! introspect through a REST surface.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use cratestack_migrate::column_name;
use serde::Serialize;

use crate::api::ApiError;
use crate::data::DataError;
use crate::data::model_info::resolve_model;
use crate::workspace::LoadedWorkspace;

#[derive(Debug, Serialize)]
pub struct DriftResponse {
    pub target: String,
    pub models: Vec<ModelDrift>,
}

#[derive(Debug, Serialize)]
pub struct ModelDrift {
    pub model: String,
    pub status: DriftStatus,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_columns: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_columns: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftStatus {
    /// Schema columns and live columns match exactly.
    Ok,
    /// Live table is missing some schema-declared columns (writes that
    /// touch those columns will fail).
    Drift,
    /// The live table doesn't exist — the migration hasn't been run.
    MissingTable,
    /// We can't tell (e.g. an API-only target). The model is reported
    /// as `unsupported` so the UI can render a muted indicator.
    Unsupported,
    /// Resolution failed earlier (no @id, unsupported PK type). The
    /// model is reported separately so the rest of the drift report
    /// still renders.
    Skipped,
}

pub async fn target_drift(
    State(state): State<Arc<LoadedWorkspace>>,
    Path(key): Path<String>,
) -> Result<Json<DriftResponse>, ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;

    let mut models = Vec::with_capacity(target.schema.models.len());
    for model_decl in &target.schema.models {
        let name = model_decl.name.clone();

        let (_, info) = match resolve_model(&target.schema, &name) {
            Ok(r) => r,
            Err(e) => {
                models.push(ModelDrift {
                    model: name,
                    status: DriftStatus::Skipped,
                    missing_columns: Vec::new(),
                    extra_columns: Vec::new(),
                    message: Some(format!("{e}")),
                });
                continue;
            }
        };

        let declared: Vec<String> = model_decl
            .fields
            .iter()
            .filter(|f| {
                !matches!(f.ty.arity, cratestack_core::TypeArity::List)
                    && !target.schema.models.iter().any(|m| m.name == f.ty.name)
            })
            .map(|f| column_name(&f.name))
            .collect();

        let live = target.source.inspect_columns(&name).await;
        match live {
            Ok(Some(observed)) => {
                let observed_names: Vec<String> = observed.iter().map(|c| c.name.clone()).collect();
                let missing: Vec<String> = declared
                    .iter()
                    .filter(|d| !observed_names.iter().any(|o| o == *d))
                    .cloned()
                    .collect();
                let extra: Vec<String> = observed_names
                    .iter()
                    .filter(|o| !declared.iter().any(|d| d == *o))
                    .cloned()
                    .collect();
                let status = if missing.is_empty() && extra.is_empty() {
                    DriftStatus::Ok
                } else {
                    DriftStatus::Drift
                };
                models.push(ModelDrift {
                    model: name,
                    status,
                    missing_columns: missing,
                    extra_columns: extra,
                    message: None,
                });
            }
            Ok(None) => {
                models.push(ModelDrift {
                    model: name,
                    status: DriftStatus::MissingTable,
                    missing_columns: declared,
                    extra_columns: Vec::new(),
                    message: Some(format!("table '{}' is not present", info.table)),
                });
            }
            Err(DataError::Unsupported { what }) => {
                models.push(ModelDrift {
                    model: name,
                    status: DriftStatus::Unsupported,
                    missing_columns: Vec::new(),
                    extra_columns: Vec::new(),
                    message: Some(what.to_owned()),
                });
            }
            Err(e) => {
                models.push(ModelDrift {
                    model: name,
                    status: DriftStatus::Skipped,
                    missing_columns: Vec::new(),
                    extra_columns: Vec::new(),
                    message: Some(format!("{e}")),
                });
            }
        }
    }

    Ok(Json(DriftResponse {
        target: target.key.clone(),
        models,
    }))
}
