//! cratestack#347 verification crate — see `README.md` and this crate's
//! `Cargo.toml` doc comment for why this exists and why it is deliberately
//! **not** a workspace member.

use cratestack::axum::Router;
use cratestack::futures::Stream;
use cratestack::futures::stream;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext};
use cratestack_codec_json::JsonCodec;

cratestack::include_server_schema!("schema.cstack", db = None);

pub use cratestack_schema as schema;

#[derive(Clone, Default)]
pub struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> Result<cratestack_schema::procedures::ping::Output, CratestackError> {
        Ok(cratestack_schema::PingReply {
            echo: args.args.message,
        })
    }

    async fn submit(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::submit::Args,
        _authorized: cratestack_schema::procedures::submit::Authorized,
    ) -> Result<cratestack_schema::procedures::submit::Output, CratestackError> {
        Ok(cratestack_schema::PingReply {
            echo: args.args.message,
        })
    }

    /// cratestack#407 follow-up: a genuinely `@stream`-shaped procedure
    /// (real `impl Stream`, not a buffered `Vec`) with a declared
    /// `@status(202)` — `tests/smoke.rs`'s
    /// `streamed_procedure_returns_the_declared_202_status` proves the
    /// declared status actually reaches this branch's HTTP response,
    /// which `procedure_dispatch_tail_tokens` previously discarded in
    /// favor of a hardcoded `StatusCode::OK`.
    fn streamed(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::streamed::Args,
        _authorized: cratestack_schema::procedures::streamed::Authorized,
    ) -> impl Stream<Item = Result<cratestack_schema::PingReply, CratestackError>> + Send {
        stream::iter([Ok(cratestack_schema::PingReply {
            echo: args.args.message,
        })])
    }
}

#[derive(Clone)]
pub struct AllowAllAuth;

impl AuthProvider for AllowAllAuth {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> Result<CratestackContext, Self::Error> {
        Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )]))
    }
}

/// `Cratestack::builder()` — zero parameters, no `PgPool`, no `sqlx` type
/// in sight, and no `cratestack-sqlx` dependency in this crate's graph to
/// have provided one even if the code wanted it. That's the whole point of
/// this crate.
pub fn build_router() -> Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::router(
        db,
        Procedures,
        (),
        JsonCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}
