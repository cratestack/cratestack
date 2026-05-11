//! Idempotency-key middleware.
//!
//! Protects mutating routes against duplicate execution. On the first request
//! with a given `Idempotency-Key`, the handler runs and the captured response
//! is persisted. Subsequent requests with the same key replay the stored
//! response if the request body hashes match, or return `422` with a
//! `idempotency_key_conflict` code if a different body is sent under the same
//! key (per the draft IETF spec).
//!
//! Usage:
//! ```ignore
//! use cratestack_axum::idempotency::{IdempotencyLayer, SqlxIdempotencyStore};
//! let store = std::sync::Arc::new(SqlxIdempotencyStore::new(pool.clone()));
//! let router = generated_router.layer(IdempotencyLayer::new(store, std::time::Duration::from_secs(24 * 3600)));
//! ```
//!
//! In Phase 1 the layer is opt-in at the consumer's router. A follow-up will
//! wire it into macro-generated routers by default, gated by a
//! `@no_idempotency` opt-out attribute already recognised by the parser.

use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_core::CoolError;
use http::{Method, StatusCode, header};
use sha2::{Digest, Sha256};
use tower::{Layer, Service};

/// Maximum body size the middleware will buffer when computing the hash. A
/// request beyond this returns 413 rather than risking unbounded memory.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Persisted idempotency record returned on a replay. Banks need an
/// invariant view of the captured response — the store rebuilds this from
/// its persisted columns when the second caller asks to replay.
#[derive(Debug, Clone)]
pub struct IdempotencyRecord {
    pub key: String,
    pub principal_fingerprint: String,
    pub request_hash: [u8; 32],
    pub response_status: u16,
    pub response_content_type: Option<String>,
    pub response_body: Vec<u8>,
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
}

/// Outcome of an atomic `reserve_or_fetch` call.
///
/// The middleware uses this state machine to decide whether to run the
/// handler, replay a cached response, or reject. Exactly one caller per
/// `(principal, key)` ever gets `Reserved` — that's the contract banking
/// flows like transfers rely on.
#[derive(Debug, Clone)]
pub enum ReservationOutcome {
    /// This caller claimed the key. It MUST run the handler and then
    /// invoke `complete` (success) or `release` (give up the
    /// reservation so a retry can re-acquire). The `token` uniquely
    /// identifies THIS reservation — `complete` and `release` only
    /// write when the row still carries the same token, so a handler
    /// that overran the TTL and had its row reclaimed by a retry
    /// can't poison the newer reservation.
    Reserved { token: uuid::Uuid },
    /// Another caller already completed an execution with the same
    /// request hash. The middleware returns the cached response.
    Replay(IdempotencyRecord),
    /// Another caller is currently executing under the same key + hash.
    /// The middleware returns `409 Conflict` with `Retry-After: 1` so
    /// the client retries shortly.
    InFlight,
    /// Same key was claimed by a different request body — the IETF
    /// draft's `idempotency_key_conflict` (422).
    Conflict,
}

#[async_trait]
pub trait IdempotencyStore: Send + Sync + 'static {
    /// Atomically reserve `(principal, key)` for the caller, or report
    /// the outcome of an existing reservation. Implementations MUST be
    /// concurrent-safe: two simultaneous callers seeing the same key and
    /// hash must observe exactly one `Reserved` and one `InFlight`,
    /// never two `Reserved`. The `expires_at` argument bounds the
    /// reservation's lifetime so a forgotten release doesn't pin the
    /// key forever; when a retry reclaims an expired row the store
    /// MUST rotate the reservation token so `complete`/`release` from
    /// the original handler can no longer touch the newer slot.
    async fn reserve_or_fetch(
        &self,
        principal: &str,
        key: &str,
        request_hash: [u8; 32],
        expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CoolError>;

    /// Persist the captured response for a previously-reserved key so
    /// subsequent attempts replay it. Banks treat the IETF idempotency
    /// contract as "freeze the outcome": if the handler returned 5xx,
    /// retries see the same 5xx unless they use a fresh key. The
    /// `token` must match the value returned by `reserve_or_fetch`
    /// when this caller claimed the key; mismatched tokens are
    /// silently no-ops so a stale handler whose reservation has been
    /// reclaimed cannot overwrite a newer execution's response.
    async fn complete(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
        status: u16,
        content_type: Option<&str>,
        body: &[u8],
    ) -> Result<(), CoolError>;

    /// Release a reservation without recording a completion (e.g. the
    /// inner service panicked or the middleware itself errored before
    /// the response was ready). Subsequent attempts with the same key
    /// can re-reserve. As with `complete`, the `token` must match the
    /// active reservation.
    async fn release(&self, principal: &str, key: &str, token: uuid::Uuid)
    -> Result<(), CoolError>;
}

/// Parse the `Idempotency-Key` request header. Returns `Ok(None)` if absent.
/// The key must be ASCII and reasonably short to avoid storage abuse.
pub fn parse_idempotency_key(headers: &http::HeaderMap) -> Result<Option<String>, CoolError> {
    let Some(value) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    let raw = value
        .to_str()
        .map_err(|_| CoolError::BadRequest("Idempotency-Key must be ASCII".to_owned()))?
        .trim();
    if raw.is_empty() {
        return Err(CoolError::BadRequest(
            "Idempotency-Key must not be empty".to_owned(),
        ));
    }
    if raw.len() > 255 {
        return Err(CoolError::BadRequest(
            "Idempotency-Key must be at most 255 characters".to_owned(),
        ));
    }
    Ok(Some(raw.to_owned()))
}

/// Stable fingerprint of a request: SHA-256 over method, path, content-type,
/// and body bytes. Used to detect when a duplicate key is reused with a
/// different payload (the conflict case the draft spec calls out).
pub fn hash_request(
    method: &Method,
    path: &str,
    content_type: Option<&str>,
    body: &[u8],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(method.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(path.as_bytes());
    hasher.update(b"\0");
    hasher.update(content_type.unwrap_or("").as_bytes());
    hasher.update(b"\0");
    hasher.update(body);
    hasher.finalize().into()
}

/// Returns true if the HTTP method is one we'd guard with idempotency. We
/// apply only to mutating verbs — GETs are already safely repeatable.
pub fn is_idempotent_target_method(method: &Method) -> bool {
    matches!(
        method,
        &Method::POST | &Method::PATCH | &Method::PUT | &Method::DELETE
    )
}

/// Tower layer that wires an `IdempotencyStore` into the request pipeline.
#[derive(Clone)]
pub struct IdempotencyLayer {
    store: Arc<dyn IdempotencyStore>,
    ttl: Duration,
    principal_fingerprint: Arc<dyn Fn(&Request) -> String + Send + Sync>,
}

impl IdempotencyLayer {
    /// Construct with a default principal fingerprint derived from the
    /// `Authorization` header. Callers running mTLS or session-cookie auth
    /// should swap this via [`with_principal_fingerprint`].
    pub fn new(store: Arc<dyn IdempotencyStore>, ttl: Duration) -> Self {
        Self {
            store,
            ttl,
            principal_fingerprint: Arc::new(default_principal_fingerprint),
        }
    }

    /// Override how the layer derives a principal-scoped namespace for the
    /// idempotency key. Without this, two callers sharing a key (across
    /// tenants) would collide.
    pub fn with_principal_fingerprint(
        mut self,
        f: impl Fn(&Request) -> String + Send + Sync + 'static,
    ) -> Self {
        self.principal_fingerprint = Arc::new(f);
        self
    }
}

fn default_principal_fingerprint(req: &Request) -> String {
    req.headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            let mut h = Sha256::new();
            h.update(s.as_bytes());
            format!("{:x}", h.finalize())
        })
        .unwrap_or_else(|| "anonymous".to_owned())
}

impl<S> Layer<S> for IdempotencyLayer {
    type Service = IdempotencyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        IdempotencyService {
            inner,
            store: self.store.clone(),
            ttl: self.ttl,
            principal_fingerprint: self.principal_fingerprint.clone(),
        }
    }
}

#[derive(Clone)]
pub struct IdempotencyService<S> {
    inner: S,
    store: Arc<dyn IdempotencyStore>,
    ttl: Duration,
    principal_fingerprint: Arc<dyn Fn(&Request) -> String + Send + Sync>,
}

impl<S> Service<Request> for IdempotencyService<S>
where
    S: Service<Request, Response = Response, Error = std::convert::Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = std::convert::Infallible;
    type Future =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let mut inner = self.inner.clone();
        let store = self.store.clone();
        let ttl = self.ttl;
        let principal_fp = self.principal_fingerprint.clone();
        Box::pin(async move {
            let method = req.method().clone();
            if !is_idempotent_target_method(&method) {
                return inner.call(req).await;
            }
            let key = match parse_idempotency_key(req.headers()) {
                Ok(Some(k)) => k,
                Ok(None) => return inner.call(req).await,
                Err(error) => return Ok(error_response(error)),
            };
            let principal = (principal_fp)(&req);
            let path = req.uri().path().to_owned();
            let content_type = req
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned());

            // Buffer the request body so we can both hash it and replay
            // it into the inner handler.
            let (parts, body) = req.into_parts();
            let bytes = match axum::body::to_bytes(body, MAX_BODY_BYTES).await {
                Ok(b) => b,
                Err(_) => {
                    return Ok(error_response(CoolError::BadRequest(
                        "request body exceeds idempotency buffer limit".to_owned(),
                    )));
                }
            };
            let hash = hash_request(&method, &path, content_type.as_deref(), &bytes);

            // Atomic reservation: exactly one caller gets `Reserved`,
            // and only then do we let the handler run. Concurrent
            // callers with the same key + same hash see `InFlight`;
            // different-body conflicts see `Conflict`. This is the
            // banking-grade duplicate-execution guarantee that the
            // previous fetch-then-put pattern could not provide.
            let expires_at = SystemTime::now() + ttl;
            let outcome = match store
                .reserve_or_fetch(&principal, &key, hash, expires_at)
                .await
            {
                Ok(outcome) => outcome,
                Err(error) => return Ok(error_response(error)),
            };

            let token = match outcome {
                ReservationOutcome::Replay(record) => {
                    return Ok(replay_response(&record));
                }
                ReservationOutcome::Conflict => {
                    return Ok(error_response(CoolError::Validation(
                        "idempotency_key_conflict: key reused with a different request body"
                            .to_owned(),
                    )));
                }
                ReservationOutcome::InFlight => {
                    return Ok(in_flight_response());
                }
                ReservationOutcome::Reserved { token } => token,
            };

            // We hold the reservation. Run the handler.
            let req2 = Request::from_parts(parts, Body::from(bytes));
            let response_result = inner.call(req2).await;
            let response = match response_result {
                Ok(response) => response,
                Err(_) => {
                    // `Service::Error = Infallible` so this branch is
                    // unreachable in practice. The release-on-error path
                    // is still here for if/when a fallible inner service
                    // is plugged in. Guarding on `token` ensures a
                    // handler whose reservation has already been
                    // reclaimed (TTL ran out) doesn't drop the new
                    // owner's row.
                    let _ = store.release(&principal, &key, token).await;
                    return Ok(error_response(CoolError::Internal(
                        "handler returned an unrecoverable error".to_owned(),
                    )));
                }
            };
            let (rparts, rbody) = response.into_parts();
            let rbytes = match axum::body::to_bytes(rbody, MAX_BODY_BYTES).await {
                Ok(b) => b,
                Err(_) => {
                    // Drop the reservation so retries can attempt
                    // again — but only if our token still holds.
                    let _ = store.release(&principal, &key, token).await;
                    let mut e = error_response(CoolError::Internal(
                        "response body exceeded idempotency buffer".to_owned(),
                    ));
                    *e.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                    return Ok(e);
                }
            };
            let content_type_header = rparts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned());

            // Persist the completion. Best-effort: on store failure we
            // still return the live response so the caller observes the
            // mutation that DID happen; banks running strict mode can
            // wrap the store in a fail-closed adapter. The `token`
            // guard means a handler whose reservation got reclaimed
            // (TTL expired, retry took over) silently fails this
            // write rather than poisoning the newer reservation's row.
            let _ = store
                .complete(
                    &principal,
                    &key,
                    token,
                    rparts.status.as_u16(),
                    content_type_header.as_deref(),
                    &rbytes,
                )
                .await;
            Ok(Response::from_parts(rparts, Body::from(rbytes)))
        })
    }
}

fn replay_response(record: &IdempotencyRecord) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::from_u16(record.response_status).unwrap_or(StatusCode::OK))
        .header("Idempotency-Replayed", "true");
    if let Some(ct) = &record.response_content_type {
        builder = builder.header(header::CONTENT_TYPE, ct.as_str());
    }
    builder
        .body(Body::from(record.response_body.clone()))
        .expect("static headers must produce a valid response")
}

/// 409 Conflict response when another request holds the reservation.
/// Banks that need a deterministic outcome should retry; `Retry-After: 1`
/// is conservative so the caller doesn't busy-loop the server.
fn in_flight_response() -> Response {
    let mut response = Response::new(Body::from(
        "another request with this Idempotency-Key is still in flight",
    ));
    *response.status_mut() = StatusCode::CONFLICT;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, http::HeaderValue::from_static("1"));
    response
}

fn error_response(error: CoolError) -> Response {
    let status = error.status_code();
    let mut response = Response::new(Body::from(error.public_message().into_owned()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

/// SQL DDL for the idempotency table. Banks typically run migrations through
/// their own tooling — `cratestack` currently ships migrations as raw DDL
/// since the migration engine is deferred to Phase 3.
pub const IDEMPOTENCY_TABLE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS cratestack_idempotency (
    principal_fingerprint TEXT NOT NULL,
    key TEXT NOT NULL,
    request_hash BYTEA NOT NULL,
    reservation_id UUID NOT NULL,
    response_status INT,
    response_content_type TEXT,
    response_body BYTEA,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (principal_fingerprint, key)
);

CREATE INDEX IF NOT EXISTS cratestack_idempotency_expires_idx
    ON cratestack_idempotency (expires_at);
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderMap;

    #[test]
    fn parses_present_and_absent_keys() {
        let mut headers = HeaderMap::new();
        assert_eq!(parse_idempotency_key(&headers).unwrap(), None);
        headers.insert("idempotency-key", http::HeaderValue::from_static("abc-123"));
        assert_eq!(
            parse_idempotency_key(&headers).unwrap(),
            Some("abc-123".to_owned())
        );
    }

    #[test]
    fn rejects_empty_key() {
        let mut headers = HeaderMap::new();
        headers.insert("idempotency-key", http::HeaderValue::from_static("   "));
        let err = parse_idempotency_key(&headers).unwrap_err();
        assert_eq!(err.code(), "BAD_REQUEST");
    }

    #[test]
    fn rejects_overlong_key() {
        let value = "a".repeat(256);
        let mut headers = HeaderMap::new();
        headers.insert(
            "idempotency-key",
            http::HeaderValue::from_bytes(value.as_bytes()).unwrap(),
        );
        let err = parse_idempotency_key(&headers).unwrap_err();
        assert_eq!(err.code(), "BAD_REQUEST");
    }

    #[test]
    fn hash_changes_with_body() {
        let a = hash_request(&Method::POST, "/transfer", Some("application/cbor"), b"{}");
        let b = hash_request(
            &Method::POST,
            "/transfer",
            Some("application/cbor"),
            b"{\"x\":1}",
        );
        assert_ne!(a, b);
    }

    #[test]
    fn hash_changes_with_method_or_path() {
        let a = hash_request(&Method::POST, "/transfer", None, b"payload");
        let b = hash_request(&Method::PATCH, "/transfer", None, b"payload");
        let c = hash_request(&Method::POST, "/credit", None, b"payload");
        assert_ne!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn idempotent_target_method_predicate_excludes_get() {
        assert!(!is_idempotent_target_method(&Method::GET));
        assert!(!is_idempotent_target_method(&Method::HEAD));
        assert!(is_idempotent_target_method(&Method::POST));
        assert!(is_idempotent_target_method(&Method::PATCH));
        assert!(is_idempotent_target_method(&Method::PUT));
        assert!(is_idempotent_target_method(&Method::DELETE));
    }
}
