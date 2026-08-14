//! Smallest possible CrateStack RPC server — two procedures, no database.
//!
//! `transport rpc` in `schema.cstack` flips the macro to emit `rpc_router`
//! instead of `model_router`/`procedure_router`. The router mounts:
//!
//! - `POST /rpc/{op_id}` — unary, content-negotiated CBOR or JSON
//! - `POST /rpc/batch`  — sequence of frames (see the rpc-batch example)
//!
//! See `tests/smoke.rs` for the wire-shape demos. The `bin/server.rs` entry
//! point starts an axum server on `127.0.0.1:3000`.
//!
//! cratestack#329: this example used to fake "no database" with a
//! `PgPoolOptions::connect_lazy` pool that was never opened. It now uses
//! the real first-class feature — `datasource { provider = "none" }` +
//! `db = None` (cratestack#327/#328) — so there is no connection string,
//! no `PgPool`, and (with this crate's `default-features = false`
//! `cratestack` dependency) no `sqlx` compiled into this binary at all.
//! See `docs/design/no-database-mode.md`.

use cratestack::axum::Router;
use cratestack::{
    AuthProvider, CodecSet, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

cratestack::include_server_schema!("schema.cstack", db = None);

// Re-export the generated module so tests + binary share one path to
// the `procedures::greet::Args`, `GreetReply`, etc. types.
pub use cratestack_schema as schema;

// cratestack#512: the documented, compiling example of calling a
// procedure correctly from non-HTTP code (a cron job here) — see that
// module's doc comment.
pub mod internal_worker;

/// In-memory counter shared across all `increment` invocations. Real
/// services would persist this — the example is about the RPC dispatch
/// shape, not the state.
#[derive(Clone, Default)]
pub struct Procedures {
    pub counter: Arc<AtomicI64>,
}

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn greet(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::greet::Args,
        _authorized: cratestack_schema::procedures::greet::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::greet::Output, CratestackError>,
    > + Send {
        async move {
            Ok(cratestack_schema::GreetReply {
                message: format!("hello, {}!", args.args.name),
            })
        }
    }

    fn increment(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::increment::Args,
        _authorized: cratestack_schema::procedures::increment::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::increment::Output, CratestackError>,
    > + Send {
        let counter = Arc::clone(&self.counter);
        async move {
            let total = counter.fetch_add(args.args.by, Ordering::Relaxed) + args.args.by;
            Ok(cratestack_schema::CounterValue { total })
        }
    }
}

/// Header-based auth provider — production code would parse JWTs / mTLS /
/// session cookies. The schema declares `@allow(auth() != null)` so we
/// only need to surface a non-anonymous context when the header is present.
#[derive(Clone)]
pub struct HeaderAuthProvider;

impl AuthProvider for HeaderAuthProvider {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let ctx = request
            .headers
            .get("x-auth-id")
            .and_then(|value| value.to_str().ok())
            .and_then(|raw| raw.parse::<i64>().ok())
            .map(|id| CratestackContext::authenticated([("id".to_owned(), Value::Int(id))]))
            .unwrap_or_else(CratestackContext::anonymous);
        core::future::ready(Ok(ctx))
    }
}

/// Build the example's RPC router. `db = None` means `Cratestack::builder()`
/// takes zero parameters — there is no `PgPool` to construct, lazily or
/// otherwise.
pub fn build_router() -> Router {
    let db = cratestack_schema::Cratestack::builder().build();

    cratestack_schema::axum::rpc_router(
        db,
        Procedures::default(),
        CodecSet::new(CborCodec, JsonCodec),
        HeaderAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}
