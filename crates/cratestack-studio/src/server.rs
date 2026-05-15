//! Phase 0 HTTP surface. Boots an Axum app that responds with a stub page
//! and a `/api/health` endpoint. Real routes (schema, records, snippet)
//! land in Phase 1.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::Router;
use axum::routing::get;
use serde::Serialize;

use crate::config::{StudioConfig, StudioConfigError};

#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub config_path: PathBuf,
    pub bind: SocketAddr,
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error(transparent)]
    Config(#[from] StudioConfigError),
    #[error("failed to bind to {address}: {source}")]
    Bind {
        address: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    #[error("server crashed: {source}")]
    Serve {
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Serialize)]
struct HealthBody {
    ok: bool,
    workspace: String,
    targets: Vec<TargetSummary>,
}

#[derive(Debug, Clone, Serialize)]
struct TargetSummary {
    key: String,
    has_db: bool,
    has_api: bool,
    mode: &'static str,
}

/// Load `studio.toml`, bind the listener, and serve until Ctrl-C.
pub async fn run(options: ServerOptions) -> Result<(), ServerError> {
    let config = StudioConfig::load(&options.config_path)?;
    let app = build_router(config.clone());

    tracing::info!(
        address = %options.bind,
        workspace = %config.workspace.name,
        targets = config.targets.len(),
        "cratestack-studio listening"
    );

    let listener = tokio::net::TcpListener::bind(options.bind)
        .await
        .map_err(|source| ServerError::Bind {
            address: options.bind,
            source,
        })?;

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|source| ServerError::Serve { source })
}

fn build_router(config: StudioConfig) -> Router {
    let health_body = HealthBody {
        ok: true,
        workspace: config.workspace.name.clone(),
        targets: config
            .targets
            .iter()
            .map(|t| TargetSummary {
                key: t.key.clone(),
                has_db: t.db.is_some(),
                has_api: t.api.is_some(),
                mode: match config.target_mode(t) {
                    crate::config::TargetMode::Ro => "ro",
                    crate::config::TargetMode::Rw => "rw",
                },
            })
            .collect(),
    };

    Router::new()
        .route("/", get(index_page))
        .route(
            "/api/health",
            get({
                let body = health_body.clone();
                move || async move { axum::Json(body) }
            }),
        )
}

async fn index_page() -> axum::response::Html<&'static str> {
    axum::response::Html(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>cratestack-studio</title></head>
<body style="font-family:system-ui;padding:2rem;max-width:42rem;margin:auto">
<h1>cratestack-studio</h1>
<p>Phase 0 stub. The full UI lands in Phase 1.</p>
<p>Sanity check the loaded config at
<a href="/api/health"><code>/api/health</code></a>.</p>
</body></html>"#,
    )
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_endpoint_lists_targets() {
        let config = StudioConfig::parse(
            r#"
                [workspace]
                name = "acme"

                [[target]]
                key = "catalog"
                schema = "schemas/catalog.cstack"
                mode = "rw"
                [target.db]
                url = "sqlite::memory:"
                driver = "sqlite"
            "#,
        )
        .expect("config should parse");

        let app = build_router(config);
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), 200);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body should read");
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).expect("body should be json");
        assert_eq!(body["workspace"], "acme");
        assert_eq!(body["targets"][0]["key"], "catalog");
        assert_eq!(body["targets"][0]["has_db"], true);
        assert_eq!(body["targets"][0]["has_api"], false);
        assert_eq!(body["targets"][0]["mode"], "rw");
    }
}
