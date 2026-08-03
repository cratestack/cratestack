//! Smallest possible `transport grpc` CrateStack server: one `Widget`
//! model, CRUD over real gRPC (ticket #171) — `ModelWidgetList/Get/
//! Create/Update/Delete` — plus two procedures over real gRPC (ticket
//! #208): `echoWidgetName` (unary) and `widgetNameSamples` (`List`-arity,
//! server-streaming). All mounted as a plain `axum::Router` via
//! `cratestack_schema::grpc::into_router` (§7.2's axum/tonic alignment,
//! exercised for real here, not just asserted).
//!
//! ### Run
//!
//! ```bash
//! export DATABASE_URL=postgres://cratestack:cratestack@localhost:55432/cratestack_test
//! cargo run -p grpc-widgets-example
//! ```
//!
//! Then, in another shell (grpcurl needs `-plaintext` — this example has
//! no TLS):
//!
//! ```bash
//! grpcurl -plaintext -import-path examples/grpc-widgets/schemas -proto widgets.proto \
//!   -H 'x-auth-id: 1' -d '{"name": "gizmo"}' \
//!   localhost:50061 widgets_api.Api/ModelWidgetCreate
//!
//! grpcurl -plaintext -import-path examples/grpc-widgets/schemas -proto widgets.proto \
//!   -H 'x-auth-id: 1' -d '{}' \
//!   localhost:50061 widgets_api.Api/ModelWidgetList
//!
//! # Ticket #208: unary procedure.
//! grpcurl -plaintext -import-path examples/grpc-widgets/schemas -proto widgets.proto \
//!   -H 'x-auth-id: 1' -d '{"name": "gizmo"}' \
//!   localhost:50061 widgets_api.Api/ProcedureEchoWidgetName
//!
//! # Ticket #208: server-streaming procedure (`List`-arity return).
//! grpcurl -plaintext -import-path examples/grpc-widgets/schemas -proto widgets.proto \
//!   -H 'x-auth-id: 1' -d '{}' \
//!   localhost:50061 widgets_api.Api/ProcedureWidgetNameSamples
//! ```

use cratestack::include_server_schema;
use cratestack::sqlx::PgPool;
use cratestack::{AuthProvider, CoolContext, CoolError, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;

include_server_schema!("schemas/widgets.cstack", db = Postgres);

/// Ticket #208: the two procedures declared in `schemas/widgets.cstack`
/// (`echoWidgetName`, a unary op; `widgetNameSamples`, a `List`-arity —
/// server-streaming — op) don't touch the database, mirroring
/// `examples/rpc-procedures`' own minimal `Procedures` registry: the
/// point of this example is proving the gRPC wiring, not business logic.
#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn echo_widget_name(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::echo_widget_name::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::echo_widget_name::Output, CoolError>,
    > + Send {
        async move { Ok(format!("echo: {}", args.name)) }
    }

    fn widget_name_samples(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        _args: cratestack_schema::procedures::widget_name_samples::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::widget_name_samples::Output, CoolError>,
    > + Send {
        async move {
            Ok(vec![
                "alpha".to_owned(),
                "beta".to_owned(),
                "gizmo".to_owned(),
            ])
        }
    }
}

/// Reads `x-auth-id` from gRPC metadata (converted to `http::HeaderMap` by
/// `cratestack::grpc::metadata_to_headers` — see `into_router`'s call
/// site) — the exact same header-driven `AuthProvider` a REST/RPC schema
/// already uses, ported unchanged per `docs/design/protobuf.md` §7.2.
#[derive(Clone)]
struct HeaderAuthProvider;

impl AuthProvider for HeaderAuthProvider {
    type Error = CoolError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        let id = request
            .headers
            .get("x-auth-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i64>().ok());
        core::future::ready(Ok(match id {
            Some(id) => CoolContext::authenticated([("id".to_owned(), Value::Int(id))]),
            None => CoolContext::anonymous(),
        }))
    }
}

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://cratestack:cratestack@localhost:55432/cratestack_test".to_owned()
    });

    let pool = PgPool::connect(&database_url)
        .await
        .expect("connect to Postgres — see DATABASE_URL");

    cratestack::sqlx::query(
        "CREATE TABLE IF NOT EXISTS widgets (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("create widgets table");

    let db = cratestack_schema::Cratestack::builder(pool).build();
    let app = cratestack_schema::grpc::into_router(db, Procedures, CborCodec, HeaderAuthProvider);

    let addr: std::net::SocketAddr = "127.0.0.1:50061".parse().expect("addr parses");
    println!("grpc-widgets-server listening on http://{addr} (h2c, plaintext gRPC)");
    println!();
    println!(
        "grpcurl -plaintext -import-path schemas -proto widgets.proto \\\n  \
         -H 'x-auth-id: 1' -d '{{\"name\": \"gizmo\"}}' \\\n  \
         localhost:50061 widgets_api.Api/ModelWidgetCreate"
    );

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind 127.0.0.1:50061");
    cratestack::axum::serve(listener, app)
        .await
        .expect("axum serve");
}
