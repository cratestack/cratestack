//! Library half of `embedded-webhook`. Exposes the axum router and the
//! sync persistence boundary so integration tests can hit it via
//! `tower::ServiceExt::oneshot` without binding a real TCP port.
//!
//! All persistence calls cross the sync/async boundary inside the
//! `tokio::task::spawn_blocking` body — `RusqliteRuntime` is `Send + Sync`
//! but blocking, so the handler defers the SQLite work to a worker.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use cratestack_macros::include_embedded_schema;
use cratestack_rusqlite::ddl::create_table_sql;
use cratestack_rusqlite::{ModelDelegate, RusqliteError, RusqliteRuntime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

include_embedded_schema!("schema.cstack");

pub use cratestack_schema::{CreateWebhookEventInput, WebhookEvent};

#[derive(Clone)]
pub struct AppState {
    pub runtime: Arc<RusqliteRuntime>,
}

pub fn bootstrap(runtime: &RusqliteRuntime) -> Result<(), RusqliteError> {
    runtime.with_connection(|conn| {
        conn.execute_batch(&create_table_sql(&cratestack_schema::WEBHOOK_EVENT_MODEL))?;
        Ok(())
    })
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/webhooks", post(create_webhook).get(list_webhooks))
        .route("/webhooks/{id}", get(get_webhook))
        .route("/webhooks/{id}/processed", post(mark_processed))
        .route("/healthz", get(healthz))
        .with_state(state)
}

#[derive(Deserialize)]
pub struct NewWebhook {
    pub source: String,
    /// Arbitrary JSON. Serialized into the row so callers can replay it.
    pub payload: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookView {
    pub id: Uuid,
    pub source: String,
    pub payload: serde_json::Value,
    pub received_at: DateTime<Utc>,
    pub status: String,
}

impl WebhookView {
    fn from_row(row: WebhookEvent) -> Self {
        let payload = serde_json::from_str(&row.payload)
            .unwrap_or_else(|_| serde_json::Value::String(row.payload.clone()));
        Self {
            id: row.id,
            source: row.source,
            payload,
            received_at: row.receivedAt,
            status: row.status,
        }
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// Filter by status. Omit to return all rows.
    pub status: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

async fn create_webhook(
    State(state): State<AppState>,
    Json(input): Json<NewWebhook>,
) -> Result<(StatusCode, Json<WebhookView>), AppError> {
    let runtime = Arc::clone(&state.runtime);
    let payload = serde_json::to_string(&input.payload).map_err(AppError::BadRequest)?;
    let row = tokio::task::spawn_blocking(move || {
        let events = ModelDelegate::new(&runtime, &cratestack_schema::WEBHOOK_EVENT_MODEL);
        events
            .create(CreateWebhookEventInput {
                id: Uuid::new_v4(),
                source: input.source,
                payload,
                receivedAt: Utc::now(),
                status: "pending".into(),
            })
            .run()
    })
    .await??;
    Ok((StatusCode::CREATED, Json(WebhookView::from_row(row))))
}

async fn list_webhooks(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<WebhookView>>, AppError> {
    let runtime = Arc::clone(&state.runtime);
    let rows = tokio::task::spawn_blocking(move || {
        let events = ModelDelegate::new(&runtime, &cratestack_schema::WEBHOOK_EVENT_MODEL);
        let mut q = events
            .find_many()
            .order_by(cratestack_schema::webhook_event::receivedAt().desc())
            .limit(query.limit);
        if let Some(status) = query.status {
            q = q.where_(cratestack_schema::webhook_event::status().eq(status));
        }
        q.run()
    })
    .await??;
    Ok(Json(rows.into_iter().map(WebhookView::from_row).collect()))
}

async fn get_webhook(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<WebhookView>, AppError> {
    let runtime = Arc::clone(&state.runtime);
    let row = tokio::task::spawn_blocking(move || {
        let events = ModelDelegate::new(&runtime, &cratestack_schema::WEBHOOK_EVENT_MODEL);
        events.find_unique(id).run()
    })
    .await??;
    row.map(WebhookView::from_row)
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn mark_processed(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<WebhookView>, AppError> {
    let runtime = Arc::clone(&state.runtime);
    let row = tokio::task::spawn_blocking(move || {
        let events = ModelDelegate::new(&runtime, &cratestack_schema::WEBHOOK_EVENT_MODEL);
        events
            .update(id)
            .set(cratestack_schema::UpdateWebhookEventInput {
                status: Some("processed".into()),
                ..Default::default()
            })
            .run()
    })
    .await??;
    Ok(Json(WebhookView::from_row(row)))
}

async fn healthz() -> &'static str {
    "ok"
}

#[derive(Debug)]
pub enum AppError {
    NotFound,
    BadRequest(serde_json::Error),
    Sqlite(RusqliteError),
    Join(tokio::task::JoinError),
}

impl From<RusqliteError> for AppError {
    fn from(value: RusqliteError) -> Self {
        Self::Sqlite(value)
    }
}

impl From<tokio::task::JoinError> for AppError {
    fn from(value: tokio::task::JoinError) -> Self {
        Self::Join(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found".to_owned()),
            AppError::BadRequest(error) => {
                (StatusCode::BAD_REQUEST, format!("bad request: {error}"))
            }
            AppError::Sqlite(error) => {
                tracing::error!(%error, "rusqlite error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal error".to_owned(),
                )
            }
            AppError::Join(error) => {
                tracing::error!(%error, "spawn_blocking task panicked");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal error".to_owned(),
                )
            }
        };
        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn build_test_state() -> AppState {
        let runtime = Arc::new(RusqliteRuntime::open_in_memory().expect("open in-memory"));
        bootstrap(&runtime).expect("bootstrap");
        AppState { runtime }
    }

    async fn json_body<T: for<'de> Deserialize<'de>>(resp: Response) -> T {
        let bytes = resp
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        serde_json::from_slice(&bytes).expect("decode json")
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        let app = build_router(build_test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("oneshot");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn create_then_list_round_trip() {
        let app = build_router(build_test_state());

        let body = serde_json::to_vec(&serde_json::json!({
            "source": "github",
            "payload": { "event": "push", "ref": "refs/heads/main" },
        }))
        .expect("encode");
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhooks")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("oneshot");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let created: WebhookView = json_body(resp).await;
        assert_eq!(created.source, "github");
        assert_eq!(created.status, "pending");

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/webhooks")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("oneshot");
        assert_eq!(resp.status(), StatusCode::OK);
        let list: Vec<WebhookView> = json_body(resp).await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, created.id);
    }

    #[tokio::test]
    async fn mark_processed_advances_status() {
        let app = build_router(build_test_state());

        let body = serde_json::to_vec(&serde_json::json!({
            "source": "stripe",
            "payload": {"type": "invoice.paid"},
        }))
        .expect("encode");
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhooks")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("oneshot");
        let created: WebhookView = json_body(resp).await;

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/webhooks/{}/processed", created.id))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("oneshot");
        assert_eq!(resp.status(), StatusCode::OK);
        let updated: WebhookView = json_body(resp).await;
        assert_eq!(updated.status, "processed");

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/webhooks?status=pending")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("oneshot");
        let pending: Vec<WebhookView> = json_body(resp).await;
        assert!(
            pending.is_empty(),
            "pending list should be empty: {pending:?}"
        );
    }

    #[tokio::test]
    async fn missing_webhook_returns_404() {
        let app = build_router(build_test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/webhooks/{}", Uuid::new_v4()))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("oneshot");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
