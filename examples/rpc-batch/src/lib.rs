//! RPC server demonstrating `POST /rpc/batch`.
//!
//! Send N requests in one round-trip, get N responses back in the same
//! order. Per-frame errors are isolated — one bad frame doesn't poison
//! the rest of the batch. See `tests/smoke.rs` for the wire shape.
//!
//! Three procedures (`add`, `multiply`, `divide`) cover the demo:
//! `divide` returns a `failed_precondition` when `denominator == 0`,
//! which exercises the per-frame error path inside the batch envelope.

use cratestack::axum::Router;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{
    AuthProvider, CodecSet, CoolContext, CoolError, RequestContext, Value,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;

cratestack::include_server_schema!("schema.cstack", db = Postgres);

pub use cratestack_schema as schema;

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
                // Maps to `failed_precondition` on the RPC binding.
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
