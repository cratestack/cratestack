//! Phase 1a HTTP surface. Boots an Axum app that loads `studio.toml`,
//! opens DB/API connections per target, and mounts the read API.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::routing::get;

use crate::workspace::{LoadedWorkspace, WorkspaceError};

#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub config_path: PathBuf,
    pub bind: SocketAddr,
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
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

/// Load `studio.toml`, materialize targets, bind the listener, serve
/// until Ctrl-C.
pub async fn run(options: ServerOptions) -> Result<(), ServerError> {
    let workspace = LoadedWorkspace::load(&options.config_path).await?;
    let app = build_router(workspace.clone());

    tracing::info!(
        address = %options.bind,
        workspace = %workspace.config.name,
        targets = workspace.targets.len(),
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

/// Public for the smoke test in `tests/api_smoke.rs`.
pub fn build_router(workspace: Arc<LoadedWorkspace>) -> Router {
    let cors_dev = workspace.config.cors_dev;
    let mut app = Router::new()
        .route("/api/health", get(health_handler))
        .merge(crate::api::router());

    // The UI is mounted *after* the API routes so any future overlap
    // (e.g. `/api/health` vs an `index.html` SPA route) resolves in
    // favor of the JSON endpoint. Without the `embed-ui` feature we
    // fall back to a stub explainer page.
    #[cfg(feature = "embed-ui")]
    {
        if crate::ui_assets::has_assets() {
            app = crate::ui_assets::mount(app);
        } else {
            tracing::warn!(
                "embed-ui feature is enabled but the UI bundle is empty; \
                 run `trunk build --release` in crates/cratestack-studio/ui/ \
                 before building the studio binary"
            );
            app = app.route("/", get(index_page));
        }
    }
    #[cfg(not(feature = "embed-ui"))]
    {
        app = app.route("/", get(index_page));
    }

    let app = app.with_state(workspace);

    if cors_dev {
        let cors = tower_http::cors::CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any);
        app.layer(cors)
    } else {
        app
    }
}

async fn index_page() -> axum::response::Html<&'static str> {
    axum::response::Html(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>cratestack-studio</title></head>
<body style="font-family:system-ui;padding:2rem;max-width:42rem;margin:auto">
<h1>cratestack-studio</h1>
<p>Phase 1a backend. The Leptos UI lands in Phase 1b.</p>
<ul>
  <li><a href="/api/health"><code>/api/health</code></a></li>
  <li><a href="/api/targets"><code>/api/targets</code></a></li>
</ul>
</body></html>"#,
    )
}

#[derive(serde::Serialize)]
struct HealthBody {
    ok: bool,
    workspace: String,
    target_count: usize,
}

async fn health_handler(
    axum::extract::State(state): axum::extract::State<Arc<LoadedWorkspace>>,
) -> axum::Json<HealthBody> {
    axum::Json(HealthBody {
        ok: true,
        workspace: state.config.name.clone(),
        target_count: state.targets.len(),
    })
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}
