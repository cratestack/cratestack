//! `GET /api/audit` — recent writes captured by the in-memory ring
//! buffer. Used by the Studio UI's audit-log tab.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};

use crate::audit::AuditEntry;
use crate::workspace::LoadedWorkspace;

#[derive(Debug, Deserialize, Default)]
pub struct AuditQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct AuditResponse {
    pub entries: Vec<AuditEntry>,
}

pub async fn list_audit(
    State(state): State<Arc<LoadedWorkspace>>,
    Query(q): Query<AuditQuery>,
) -> Json<AuditResponse> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let entries = state.audit.snapshot(limit);
    Json(AuditResponse { entries })
}
