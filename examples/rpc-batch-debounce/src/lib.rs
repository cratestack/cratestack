//! Client-side **batch debouncer** for the RPC binding.
//!
//! Many independent `.call(op, input)` invocations are coalesced into a
//! single `POST /rpc/batch` round-trip. The common pattern for:
//!
//! - Offline-first UIs flushing their outbox after reconnecting.
//! - Rate-sensitive callers (e.g. mobile clients on metered networks).
//! - Server-to-server callers that want N tiny ops to share one TCP
//!   round-trip without writing batch envelopes by hand.
//!
//! ## Shape
//!
//! Each `.call(op, input).await` looks like a normal request from the
//! caller's perspective. Internally the debouncer:
//!
//! 1. Pushes a `(frame, oneshot::Sender)` pair onto a pending buffer.
//! 2. Returns the `oneshot::Receiver` as a future.
//! 3. Auto-flushes when the buffer hits `max_size` (or when the user
//!    calls `.flush()` explicitly).
//! 4. On flush: builds one `POST /rpc/batch` body, sends it, decodes the
//!    response, splits each `RpcResponseFrame` back to its waiter.
//!
//! This file is the **`BatchDebouncer` itself**; see `main.rs` for an
//! end-to-end demo against a local server, and `tests/smoke.rs` for the
//! deterministic in-process tests.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cratestack::axum::Router;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::rpc::{RpcRequest, RpcResponseFrame};
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{
    AuthProvider, CodecSet, CoolCodec, CoolContext, CoolError, RequestContext, Value,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use tokio::sync::{Mutex, oneshot};
use tower::ServiceExt;

cratestack::include_server_schema!("schema.cstack", db = Postgres);

pub use cratestack_schema as schema;

// -----------------------------------------------------------------------------
// Server side — identical to the rpc-batch example, duplicated here so this
// crate stays self-contained. In production you'd point the debouncer at any
// RPC server; the schema lives wherever it lives.
// -----------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn add(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::add::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::add::Output, CoolError>,
    > + Send {
        async move {
            Ok(cratestack_schema::ScalarResult {
                value: args.args.a + args.args.b,
            })
        }
    }

    fn multiply(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::multiply::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::multiply::Output, CoolError>,
    > + Send {
        async move {
            Ok(cratestack_schema::ScalarResult {
                value: args.args.a * args.args.b,
            })
        }
    }

    fn divide(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::divide::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::divide::Output, CoolError>,
    > + Send {
        async move {
            if args.args.denominator == 0 {
                return Err(CoolError::PreconditionFailed(
                    "denominator must not be zero".to_owned(),
                ));
            }
            Ok(cratestack_schema::ScalarResult {
                value: args.args.numerator / args.args.denominator,
            })
        }
    }
}

#[derive(Clone)]
pub struct HeaderAuthProvider;

impl AuthProvider for HeaderAuthProvider {
    type Error = CoolError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        let ctx = request
            .headers
            .get("x-auth-id")
            .and_then(|value| value.to_str().ok())
            .and_then(|raw| raw.parse::<i64>().ok())
            .map(|id| CoolContext::authenticated([("id".to_owned(), Value::Int(id))]))
            .unwrap_or_else(CoolContext::anonymous);
        core::future::ready(Ok(ctx))
    }
}

pub fn build_router() -> Router {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://example:example@localhost/example".to_owned());
    let pool = PgPoolOptions::new()
        .connect_lazy(&url)
        .expect("connect_lazy parses the URL but opens no socket");
    let db = cratestack_schema::Cratestack::builder(pool).build();

    cratestack_schema::axum::rpc_router(
        db,
        Procedures,
        CodecSet::new(CborCodec, JsonCodec),
        HeaderAuthProvider,
    )
}

// -----------------------------------------------------------------------------
// Client side — the actual debouncer
// -----------------------------------------------------------------------------

/// Errors the debouncer can surface to a caller separate from per-frame
/// errors carried inside `RpcResponseFrame.error`. These are
/// infrastructure-level: the batch HTTP request itself failed, the
/// response wouldn't decode, etc.
#[derive(Debug)]
pub enum DebouncerError {
    /// The batch HTTP request failed at the transport layer.
    Transport(String),
    /// The batch response wouldn't decode as `Vec<RpcResponseFrame>`.
    DecodeFailed(String),
    /// `flush()` was called but `call()` is still racing — caller awaits
    /// were dropped before their results arrived.
    Cancelled,
}

impl core::fmt::Display for DebouncerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transport(s) => write!(f, "batch transport error: {s}"),
            Self::DecodeFailed(s) => write!(f, "batch response decode failed: {s}"),
            Self::Cancelled => write!(f, "debouncer call was cancelled"),
        }
    }
}

impl std::error::Error for DebouncerError {}

struct PendingFrame {
    id: u64,
    op: String,
    input: serde_json::Value,
    responder: oneshot::Sender<RpcResponseFrame>,
}

/// A client-side debouncer for the RPC batch route.
///
/// **Auto-flush triggers**:
/// - The pending buffer reaches `max_size` (size-based).
/// - The caller explicitly calls [`Self::flush`].
///
/// No time-based auto-flush is built in to keep the type deterministic
/// for tests. If you want one, spawn a tokio task that calls
/// `debouncer.flush()` on an interval — see `main.rs`.
#[derive(Clone)]
pub struct BatchDebouncer {
    inner: Arc<Inner>,
}

struct Inner {
    service: Router,
    auth_header: Option<String>,
    max_size: usize,
    pending: Mutex<Vec<PendingFrame>>,
    next_id: AtomicU64,
}

impl BatchDebouncer {
    pub fn new(service: Router, max_size: usize) -> Self {
        assert!(max_size > 0, "max_size must be positive");
        Self {
            inner: Arc::new(Inner {
                service,
                auth_header: None,
                max_size,
                pending: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
            }),
        }
    }

    pub fn with_auth_id(self, id: i64) -> Self {
        let inner = Arc::try_unwrap(self.inner).unwrap_or_else(|arc| {
            // Cheap path for the common case where the builder is the
            // sole owner. If a clone exists, panic — building should
            // happen before any clones are taken.
            panic!("with_auth_id called after BatchDebouncer was cloned: {arc:p}")
        });
        Self {
            inner: Arc::new(Inner {
                auth_header: Some(id.to_string()),
                ..inner
            }),
        }
    }

    /// Queue an op for the next batch flush. Returns a future that
    /// resolves to the per-frame response when the batch round-trip
    /// completes. If the buffer hits `max_size`, an auto-flush is
    /// scheduled before this returns.
    pub async fn call(
        &self,
        op: impl Into<String>,
        input: serde_json::Value,
    ) -> Result<RpcResponseFrame, DebouncerError> {
        let (tx, rx) = oneshot::channel();
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let frame = PendingFrame {
            id,
            op: op.into(),
            input,
            responder: tx,
        };

        let should_flush = {
            let mut pending = self.inner.pending.lock().await;
            pending.push(frame);
            pending.len() >= self.inner.max_size
        };

        if should_flush {
            self.flush().await?;
        }

        rx.await.map_err(|_| DebouncerError::Cancelled)
    }

    /// Drain the pending buffer and send everything as one
    /// `POST /rpc/batch`. Safe to call when the buffer is empty (no-op).
    pub async fn flush(&self) -> Result<(), DebouncerError> {
        let drained: Vec<PendingFrame> = {
            let mut pending = self.inner.pending.lock().await;
            std::mem::take(&mut *pending)
        };

        if drained.is_empty() {
            return Ok(());
        }

        // Map of correlation id → responder, then build the wire batch.
        let mut responders: std::collections::HashMap<u64, oneshot::Sender<RpcResponseFrame>> =
            std::collections::HashMap::with_capacity(drained.len());
        let mut frames: Vec<RpcRequest> = Vec::with_capacity(drained.len());
        for pending in drained {
            responders.insert(pending.id, pending.responder);
            frames.push(RpcRequest {
                id: pending.id,
                op: pending.op,
                input: pending.input,
                idem: None,
            });
        }

        let body = CborCodec
            .encode(&frames)
            .map_err(|e| DebouncerError::Transport(format!("encode batch body: {e}")))?;

        let mut builder =
            Request::post("/rpc/batch").header("content-type", CborCodec::CONTENT_TYPE);
        if let Some(auth) = &self.inner.auth_header {
            builder = builder.header("x-auth-id", auth);
        }
        let request = builder
            .body(Body::from(body))
            .map_err(|e| DebouncerError::Transport(format!("build request: {e}")))?;

        let response = self
            .inner
            .service
            .clone()
            .oneshot(request)
            .await
            .map_err(|e| DebouncerError::Transport(format!("oneshot: {e}")))?;

        if response.status() != StatusCode::OK {
            return Err(DebouncerError::Transport(format!(
                "batch envelope returned {}",
                response.status(),
            )));
        }

        let body_bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .map_err(|e| DebouncerError::Transport(format!("buffer body: {e}")))?;

        let decoded: Vec<RpcResponseFrame> = CborCodec
            .decode(&body_bytes)
            .map_err(|e| DebouncerError::DecodeFailed(format!("{e}")))?;

        // Fan responses back out to the awaiting callers by correlation id.
        for frame in decoded {
            if let Some(responder) = responders.remove(&frame.id) {
                // If the caller dropped their future before we got here,
                // send returns Err — it's fine, no awaiter to notify.
                let _ = responder.send(frame);
            }
        }

        // Any leftover responders correspond to ids the server didn't
        // echo. Should be impossible if the server is well-formed, but
        // surface the failure rather than hanging the await.
        for (_, responder) in responders {
            // Sending an error frame is the simplest way to wake the
            // awaiter; the caller can decide what to do.
            let synthetic = RpcResponseFrame {
                id: 0,
                output: None,
                error: Some(cratestack::rpc::RpcErrorBody {
                    code: "internal".to_owned(),
                    message: "server omitted this frame from the batch response".to_owned(),
                    details: None,
                }),
            };
            let _ = responder.send(synthetic);
        }

        Ok(())
    }

    /// Number of ops queued but not yet flushed. Useful in tests +
    /// telemetry.
    pub async fn pending_len(&self) -> usize {
        self.inner.pending.lock().await.len()
    }
}
