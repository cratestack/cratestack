//! A `RegistryVerifierResolver` behind the envelope layer of a generated
//! router (cratestack#1149): a key registered while the server runs is
//! accepted, a revoked one and a never-registered one get the unsigned `401`.
//! The registry sits below the transport, so REST and RPC need no separate
//! test: both layers call the same `CoseVerifierResolver` (parity: n/a).

mod cose_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cose_support::*;
use cratestack::axum::Router;
use cratestack::axum::http::StatusCode;
use cratestack::cose::RegistryVerifierResolver;
use cratestack::{CratestackCodec, CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;

include_server_schema!("tests/fixtures/no_database_procedures.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures(Arc<AtomicUsize>);

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CratestackError>,
    > + Send {
        self.0.fetch_add(1, Ordering::SeqCst);
        async move {
            Ok(cratestack_schema::PingReply {
                echo: args.args.message,
            })
        }
    }
}

fn everyone(_: &cratestack::axum::http::HeaderMap) -> Result<CratestackContext, CratestackError> {
    Ok(CratestackContext::authenticated([(
        "id".to_owned(),
        cratestack::Value::Int(1),
    )]))
}

fn router(registry: &Arc<RegistryVerifierResolver>, procedures: &Procedures) -> Router {
    let layer = envelope_layer_over(
        server_envelope_with(registry.clone()),
        cratestack_schema::ACCEPTED_CONTRACTS,
    )
    .rest("", cratestack_schema::axum::ROUTE_TRANSPORTS)
    .build()
    .expect("layer");
    cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        procedures.clone(),
        (),
        CborCodec,
        everyone,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(layer)
}

const PING: Call = Call {
    route: "/$procs/ping",
    contracts: cratestack_schema::OP_CONTRACTS,
};

/// One signed ping; the client's `cti` is random, so no call is a replay.
async fn ping(router: &Router, message: &str) -> Answer {
    let payload = CborCodec
        .encode(&serde_json::json!({ "args": { "message": message } }))
        .expect("encode");
    let (_, req) = PING.request("/$procs/ping", &payload, &[]).await;
    send(router, req).await
}

#[tokio::test]
async fn registered_at_runtime_then_revoked() {
    let registry = Arc::new(RegistryVerifierResolver::new());
    let procedures = Procedures::default();
    let router = router(&registry, &procedures);

    // Never registered: refused before the handler, with the router's
    // unsigned 401.
    let unknown = ping(&router, "m0").await;
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);
    assert!(!unknown.is_sealed());
    assert_eq!(procedures.0.load(Ordering::SeqCst), 0);

    // Registered while the router is serving: the very next call runs.
    let key = client_verify_key();
    let kid = registry.register(key.clone()).expect("register");
    let accepted = ping(&router, "m1").await;
    assert_eq!(accepted.status, StatusCode::OK);
    assert!(accepted.is_sealed());
    assert_eq!(procedures.0.load(Ordering::SeqCst), 1);

    // Registering again changes nothing.
    assert_eq!(registry.register(key).expect("again"), kid);
    assert_eq!(registry.len(), 1);

    // Revoked: the same client is refused again, and the handler never ran.
    assert_eq!(registry.revoke(&kid), 1);
    let revoked = ping(&router, "m2").await;
    assert_eq!(revoked.status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&revoked.body), error_code(&unknown.body));
    assert_eq!(procedures.0.load(Ordering::SeqCst), 1);
}
