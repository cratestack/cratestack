//! `GET /api/targets` — list configured targets and their capabilities.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::config::TargetMode;
use crate::workspace::LoadedWorkspace;

#[derive(Debug, Serialize)]
pub struct TargetSummary {
    pub key: String,
    pub display_name: String,
    pub mode: &'static str,
    pub has_db: bool,
    pub has_api: bool,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub workspace: String,
    pub targets: Vec<TargetSummary>,
}

pub async fn list_targets(State(state): State<Arc<LoadedWorkspace>>) -> Json<ListResponse> {
    let targets = state
        .targets
        .iter()
        .map(|t| TargetSummary {
            key: t.key.clone(),
            display_name: t.display_name.clone(),
            mode: mode_to_str(t.mode),
            has_db: t.has_db,
            has_api: t.has_api,
        })
        .collect();
    Json(ListResponse {
        workspace: state.config.name.clone(),
        targets,
    })
}

fn mode_to_str(mode: TargetMode) -> &'static str {
    match mode {
        TargetMode::Ro => "ro",
        TargetMode::Rw => "rw",
    }
}
