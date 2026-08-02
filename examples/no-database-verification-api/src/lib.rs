//! cratestack#347 verification crate — see `README.md` and this crate's
//! `Cargo.toml` doc comment for why this exists and why it is deliberately
//! **not** a workspace member.

use cratestack::axum::Router;
use cratestack::{AuthProvider, CoolContext, CoolError, RequestContext};
use cratestack_codec_json::JsonCodec;

cratestack::include_server_schema!("schema.cstack", db = None);

pub use cratestack_schema as schema;

#[derive(Clone, Default)]
pub struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::ping::Args,
    ) -> impl core::future::Future<Output = Result<cratestack_schema::procedures::ping::Output, CoolError>>
    + Send {
        async move {
            Ok(cratestack_schema::PingReply {
                echo: args.args.message,
            })
        }
    }
}

#[derive(Clone)]
pub struct AllowAllAuth;

impl AuthProvider for AllowAllAuth {
    type Error = CoolError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        core::future::ready(Ok(CoolContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

/// `Cratestack::builder()` — zero parameters, no `PgPool`, no `sqlx` type
/// in sight, and no `cratestack-sqlx` dependency in this crate's graph to
/// have provided one even if the code wanted it. That's the whole point of
/// this crate.
pub fn build_router() -> Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::router(db, Procedures, JsonCodec, AllowAllAuth)
}
