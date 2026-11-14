//! One schema, three providers (cratestack#1245).
//!
//! [`app`] is the whole service: the generated REST router for
//! `schema.cstack`, with the procedures below, and whatever
//! [`AuthProvider`] it is handed. The three providers in [`providers`] are
//! the only difference between the three binaries, and the shared test suite
//! (`tests/support`) runs unchanged against each.

pub mod no_devices;
pub mod providers;

use cratestack::axum::Router;
use cratestack::{AuthProvider, CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_json::JsonCodec;

include_server_schema!("schema.cstack", db = None);

/// The procedures. They read the caller through the principal contract
/// (ADR 0021 §2) only, so they never know which provider ran.
#[derive(Clone, Default)]
pub struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn who_am_i(
        &self,
        _db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        _args: cratestack_schema::procedures::who_am_i::Args,
        _authorized: cratestack_schema::procedures::who_am_i::Authorized,
    ) -> Result<cratestack_schema::Identity, CratestackError> {
        Ok(cratestack_schema::Identity {
            subject: subject(ctx),
            roles: ctx
                .roles()
                .unwrap_or_default()
                .into_iter()
                .map(str::to_owned)
                .collect(),
        })
    }

    async fn admin_report(
        &self,
        _db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        _args: cratestack_schema::procedures::admin_report::Args,
        _authorized: cratestack_schema::procedures::admin_report::Authorized,
    ) -> Result<cratestack_schema::Report, CratestackError> {
        Ok(cratestack_schema::Report {
            subject: subject(ctx),
            note: "only callers with the admin role see this".to_owned(),
        })
    }
}

fn subject(ctx: &CratestackContext) -> String {
    ctx.subject().map(String::from).unwrap_or_default()
}

/// The service, authenticated by `auth`.
pub fn app<A: AuthProvider>(auth: A) -> Router {
    cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        JsonCodec,
        auth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

/// Serves [`app`] on `addr` until the process is stopped.
pub async fn serve<A: AuthProvider>(auth: A, addr: &str) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("listening on http://{}", listener.local_addr()?);
    cratestack::axum::serve(listener, app(auth)).await
}
