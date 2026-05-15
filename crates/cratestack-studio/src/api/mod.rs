//! HTTP routes exposed by the Studio server. Phase 1a ships the
//! read path:
//!
//! - `GET /api/targets` — list of configured targets + capabilities
//! - `GET /api/targets/:key/schema` — `OwnedSchemaSummary` for the target
//! - `GET /api/targets/:key/models` — model summaries (name, fields, pk)
//! - `GET /api/targets/:key/models/:m/records?cursor=…&limit=…`
//! - `GET /api/targets/:key/models/:m/records/:pk`
//! - `GET /api/targets/:key/models/:m/snippet?pk=…`

mod errors;
mod records;
mod schema;
mod snippet;
mod targets;

use std::sync::Arc;

use axum::Router;
use axum::routing::get;

use crate::workspace::LoadedWorkspace;

pub use errors::ApiError;

/// Build the `/api/...` router. Returned with the workspace state
/// still unbound so the caller can `.merge` it with sibling routes
/// (e.g. `/`, `/api/health`) and call `.with_state` once at the top.
pub fn router() -> Router<Arc<LoadedWorkspace>> {
    Router::new()
        .route("/api/targets", get(targets::list_targets))
        .route("/api/targets/{key}/schema", get(schema::target_schema))
        .route("/api/targets/{key}/models", get(schema::list_models))
        .route(
            "/api/targets/{key}/models/{model}/records",
            get(records::list_records),
        )
        .route(
            "/api/targets/{key}/models/{model}/records/{pk}",
            get(records::get_record),
        )
        .route(
            "/api/targets/{key}/models/{model}/snippet",
            get(snippet::record_snippet),
        )
}
