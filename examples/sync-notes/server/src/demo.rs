//! Demo-page plumbing: static files, and two **read-only, unauthenticated
//! inspection endpoints** the page's "server" panel polls so you can watch
//! Postgres and the change log while you click. They exist to make the
//! example legible; a real deployment has neither.

use std::path::{Component, Path, PathBuf};

use cratestack::axum::extract::{Path as UrlPath, State};
use cratestack::axum::http::{StatusCode, header};
use cratestack::axum::response::{IntoResponse, Response};
use cratestack::axum::routing::get;
use cratestack::axum::{Json, Router};
use cratestack::sqlx::{self, PgPool};
use serde_json::{Value, json};

#[derive(Clone)]
struct DemoState {
    pool: PgPool,
    web_dir: PathBuf,
}

pub fn router(pool: PgPool, web_dir: PathBuf) -> Router {
    Router::new()
        .route("/demo/server", get(server_state))
        .route("/", get(index))
        .route("/{*path}", get(asset))
        .with_state(DemoState { pool, web_dir })
}

async fn server_state(State(state): State<DemoState>) -> Response {
    let notes: Result<Vec<(String, String, String, bool)>, _> = sqlx::query_as(
        "SELECT id::text, owner_id, title, done FROM notes ORDER BY owner_id, updated_at",
    )
    .fetch_all(&state.pool)
    .await;
    let changes: Result<Vec<(String, String, String, String, String)>, _> = sqlx::query_as(
        "SELECT scope, model, pk, op, txid::text || '.' || seq FROM cratestack_sync_changes \
         ORDER BY txid DESC, seq DESC LIMIT 12",
    )
    .fetch_all(&state.pool)
    .await;
    let clients: Result<Vec<(String, String, i64)>, _> = sqlx::query_as(
        "SELECT client_id::text, principal, last_mutation_seq FROM cratestack_sync_clients \
         ORDER BY created_at",
    )
    .fetch_all(&state.pool)
    .await;
    match (notes, changes, clients) {
        (Ok(notes), Ok(changes), Ok(clients)) => Json(json!({
            "notes": notes.into_iter().map(|(id, owner, title, done)|
                json!({"id": id, "ownerId": owner, "title": title, "done": done})).collect::<Vec<Value>>(),
            "changes": changes.into_iter().map(|(scope, model, pk, op, position)|
                json!({"scope": scope, "model": model, "pk": pk, "op": op, "position": position})).collect::<Vec<Value>>(),
            "clients": clients.into_iter().map(|(id, principal, seq)|
                json!({"clientId": id, "principal": principal, "lastMutationSeq": seq})).collect::<Vec<Value>>(),
        }))
        .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn index(State(state): State<DemoState>) -> Response {
    serve(&state.web_dir, Path::new("index.html")).await
}

async fn asset(State(state): State<DemoState>, UrlPath(path): UrlPath<String>) -> Response {
    let path = Path::new(&path);
    // Plain relative paths only: no `..`, no absolute paths, no prefixes.
    if !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return StatusCode::NOT_FOUND.into_response();
    }
    serve(&state.web_dir, path).await
}

async fn serve(root: &Path, relative: &Path) -> Response {
    let Ok(bytes) = tokio::fs::read(root.join(relative)).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = match relative.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    };
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes,
    )
        .into_response()
}
