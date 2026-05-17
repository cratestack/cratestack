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

use cratestack::axum::Router;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{AuthProvider, CodecSet, CoolContext, CoolError, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

cratestack::include_server_schema!("schema.cstack", db = Postgres);

// Re-export the generated module so tests + binary share one path to
// the `procedures::greet::Args`, `GreetReply`, etc. types.
pub use cratestack_schema as schema;

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
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::greet::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::greet::Output, CoolError>,
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
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::increment::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::increment::Output, CoolError>,
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

/// Build the example's RPC router. `connect_lazy` means we never open a
/// real DB connection — the example's procedures don't touch the DB.
pub fn build_router() -> Router {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://example:example@localhost/example".to_owned());
    let pool = PgPoolOptions::new()
        .connect_lazy(&url)
        .expect("connect_lazy parses the URL but opens no socket");
    let db = cratestack_schema::Cratestack::builder(pool).build();

    cratestack_schema::axum::rpc_router(
        db,
        Procedures::default(),
        CodecSet::new(CborCodec, JsonCodec),
        HeaderAuthProvider,
    )
}
