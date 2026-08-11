//! HTTP handlers for draining and garbage-collecting the outbox. Mount
//! these behind whatever request-authentication middleware the service
//! already uses for internal/service-to-service calls — this crate has no
//! opinion on auth and takes no dependency on one.

use axum::{
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::client::OutboxClient;
use crate::drain::DrainRequest;
use crate::negotiate::{respond, respond_error};

pub use crate::negotiate::decode_body;

/// Default GC retention: 14 days.
const DEFAULT_GC_RETENTION_SECONDS: i64 = 14 * 24 * 60 * 60;
/// Hard floor — disallow a misconfigured or malicious caller from wiping
/// the table by passing a tiny retention window.
const MIN_GC_RETENTION_SECONDS: i64 = 60 * 60;

/// Handler for the events-drain endpoint. Accepts a JSON or CBOR
/// [`DrainRequest`] body (content-negotiated per `Content-Type`; empty body
/// defaults to [`DrainRequest::default`]) and responds with a
/// [`crate::DrainResponse`], JSON or CBOR per `Accept`.
pub async fn drain_handler(State(client): State<OutboxClient>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    let body = match to_bytes(body, 1_024 * 1_024).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return respond_error(
                &headers,
                StatusCode::BAD_REQUEST,
                "invalid_body",
                &format!("could not read request body: {err}"),
            );
        }
    };
    let req: DrainRequest = match decode_body(&headers, &body) {
        Ok(value) => value,
        Err(message) => {
            return respond_error(&headers, StatusCode::BAD_REQUEST, "invalid_body", &message);
        }
    };
    match client.drain(&req).await {
        Ok(response) => respond(&headers, StatusCode::OK, &response),
        Err(err) => respond_error(
            &headers,
            StatusCode::INTERNAL_SERVER_ERROR,
            "drain_failed",
            &err.to_string(),
        ),
    }
}

/// Request payload for [`gc_handler`]. Defaults to a 14-day retention
/// window; callers can override to run a tighter (or wider) sweep without
/// redeploying the emitter.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct GcRequest {
    pub older_than_seconds: i64,
}

impl Default for GcRequest {
    fn default() -> Self {
        Self {
            older_than_seconds: DEFAULT_GC_RETENTION_SECONDS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcResponse {
    pub deleted: u64,
    pub cutoff: chrono::DateTime<Utc>,
}

/// Handler for the events-GC endpoint. Deletes events where `occurred_at <
/// now() - older_than_seconds`, clamped to a 1-hour minimum so a
/// misconfigured caller cannot wipe the table.
pub async fn gc_handler(State(client): State<OutboxClient>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    let body = match to_bytes(body, 64 * 1_024).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return respond_error(
                &headers,
                StatusCode::BAD_REQUEST,
                "invalid_body",
                &format!("could not read request body: {err}"),
            );
        }
    };
    let req: GcRequest = match decode_body(&headers, &body) {
        Ok(value) => value,
        Err(message) => {
            return respond_error(&headers, StatusCode::BAD_REQUEST, "invalid_body", &message);
        }
    };
    let older_than = req.older_than_seconds.max(MIN_GC_RETENTION_SECONDS);
    let cutoff = Utc::now() - chrono::Duration::seconds(older_than);
    match client.gc_older_than(cutoff).await {
        Ok(deleted) => respond(&headers, StatusCode::OK, &GcResponse { deleted, cutoff }),
        Err(err) => respond_error(
            &headers,
            StatusCode::INTERNAL_SERVER_ERROR,
            "gc_failed",
            &err.to_string(),
        ),
    }
}
