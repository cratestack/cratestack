//! `CoseAuthProvider` in front of a generated `transport rpc` router, behind
//! the COSE envelope layer (cratestack#1077). The guarded op is `procedure.ping`
//! of `rpc_batch_no_database.cstack`, `@allow(auth() != null)`. Its REST twin is
//! `cose_auth_provider_rest.rs`, and the two must stay in step.

mod cose_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cose_support::*;
use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode, header};
use cratestack::cose::{CoseVerifierResolver, RegistryVerifierResolver, StaticVerifierResolver};
use cratestack::envelope_layer::{EnvelopeLayer, EnvelopeMode};
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, include_server_schema,
};
use cratestack_codec_cbor::CborCodec;

include_server_schema!("tests/fixtures/rpc_batch_no_database.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures(Arc<AtomicUsize>);

impl Procedures {
    fn runs(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CratestackError>,
    > + Send {
        self.0.fetch_add(1, Ordering::SeqCst);
        let who = principal_id(ctx);
        async move {
            Ok(cratestack_schema::PingReply {
                nonce: format!("{}|{who}", args.args.nonce),
            })
        }
    }
}

fn router<A: AuthProvider>(auth: A, procedures: Procedures, envelope: EnvelopeLayer) -> Router {
    cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder().build(),
        procedures,
        (),
        CborCodec,
        auth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(envelope)
}

/// The envelope, trusting the keys `trusted` holds, in `mode`.
fn envelope(trusted: Arc<dyn CoseVerifierResolver>, mode: EnvelopeMode) -> EnvelopeLayer {
    EnvelopeLayer::builder(
        server_envelope_with(trusted),
        AUDIENCE,
        cratestack_schema::ACCEPTED_CONTRACTS,
    )
    .policy(mode)
    .rpc("")
    .build()
    .expect("layer")
}

const PING: Call = Call {
    route: "procedure.ping",
    contracts: cratestack_schema::OP_CONTRACTS,
};

fn ping_payload(nonce: &str) -> Vec<u8> {
    CborCodec
        .encode(&serde_json::json!({ "args": { "nonce": nonce } }))
        .expect("encode")
}

/// One signed call with `extra` headers, and its answer.
async fn ping(router: &Router, nonce: &str, extra: &[(&str, &str)]) -> (Sent, Answer) {
    let (sent, req) = PING
        .request("/rpc/procedure.ping", &ping_payload(nonce), extra)
        .await;
    (sent, send(router, req).await)
}

/// The payload of an answer that verifies, as the reply type.
async fn reply(sent: &Sent, answer: &Answer) -> cratestack_schema::PingReply {
    assert_eq!(answer.status, StatusCode::OK);
    CborCodec
        .decode(&PING.open(sent, answer).await.expect("verifies"))
        .expect("decode")
}

/// The code of a signed refusal, which is sealed like any answer to a signed
/// request.
async fn refusal_code(sent: &Sent, answer: &Answer) -> String {
    error_code(&PING.open(sent, answer).await.expect("a signed refusal"))
}

#[tokio::test]
async fn a_cose_only_call_authenticates_through_the_registry() {
    let registry = Arc::new(RegistryVerifierResolver::new());
    registry.register(client_verify_key()).expect("register");
    let procedures = Procedures::default();
    let router = router(
        cose_auth(registry.clone()),
        procedures.clone(),
        envelope(registry.clone(), EnvelopeMode::Required),
    );

    // No `Authorization`. A forged signer header naming someone else changes
    // nothing: the principal is the verified key's thumbprint.
    let (sent, answer) = ping(
        &router,
        "n1",
        &[("cratestack-verified-signer", "cose:ffff")],
    )
    .await;

    assert_eq!(
        reply(&sent, &answer).await.nonce,
        format!("n1|{CLIENT_PRINCIPAL}")
    );
    assert_eq!(procedures.runs(), 1);
}

#[tokio::test]
async fn a_key_revoked_in_the_shared_registry_is_refused_by_the_envelope() {
    let registry = Arc::new(RegistryVerifierResolver::new());
    let kid = registry.register(client_verify_key()).expect("register");
    let procedures = Procedures::default();
    let router = router(
        cose_auth(registry.clone()),
        procedures.clone(),
        envelope(registry.clone(), EnvelopeMode::Required),
    );
    assert_eq!(ping(&router, "n1", &[]).await.1.status, StatusCode::OK);

    assert_eq!(registry.revoke(&kid), 1);

    let (_, refused) = ping(&router, "n2", &[]).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert!(!refused.is_sealed(), "the envelope's unsigned 401");
    assert_eq!(error_code(&refused.body), "unauthenticated");
    assert_eq!(procedures.runs(), 1, "the handler never ran");
}

#[tokio::test]
async fn a_key_revoked_only_in_the_device_registry_is_refused_by_policy() {
    // The envelope trusts the key for good; only the device registry revokes
    // it. So the refusal comes from the adapter, not from the envelope.
    let trusted = Arc::new(StaticVerifierResolver::new().with_key(client_verify_key()));
    let devices = Arc::new(RegistryVerifierResolver::new());
    let kid = devices.register(client_verify_key()).expect("register");
    let procedures = Procedures::default();
    let router = router(
        cose_auth(devices.clone()),
        procedures.clone(),
        envelope(trusted, EnvelopeMode::Required),
    );
    let (sent, accepted) = ping(&router, "n1", &[]).await;
    assert_eq!(
        reply(&sent, &accepted).await.nonce,
        format!("n1|{CLIENT_PRINCIPAL}")
    );

    assert_eq!(devices.revoke(&kid), 1);

    let (sent, denied) = ping(&router, "n2", &[]).await;
    assert_eq!(refusal_code(&sent, &denied).await, "permission_denied");
    assert_eq!(procedures.runs(), 1, "policy refused before the handler");
}

#[tokio::test]
async fn a_verified_key_the_device_registry_never_held_is_anonymous() {
    let trusted = Arc::new(StaticVerifierResolver::new().with_key(client_verify_key()));
    let procedures = Procedures::default();
    let router = router(
        cose_auth(Arc::new(RegistryVerifierResolver::new())),
        procedures.clone(),
        envelope(trusted, EnvelopeMode::Required),
    );

    let (sent, denied) = ping(&router, "n1", &[]).await;

    assert_eq!(refusal_code(&sent, &denied).await, "permission_denied");
    assert_eq!(procedures.runs(), 0);
}

#[tokio::test]
async fn a_forged_signer_on_an_unsigned_request_does_not_authenticate() {
    // `Optional` lets the unsigned request reach the handler, so the adapter
    // alone decides. The client's real thumbprint is registered, and it is
    // sent in headers the way a signer would be named.
    let devices = Arc::new(RegistryVerifierResolver::new());
    devices.register(client_verify_key()).expect("register");
    let trusted = Arc::new(StaticVerifierResolver::new().with_key(client_verify_key()));
    let procedures = Procedures::default();
    let router = router(
        cose_auth(devices),
        procedures.clone(),
        envelope(trusted, EnvelopeMode::Optional),
    );
    let thumbprint = CLIENT_PRINCIPAL.trim_start_matches("cose:");
    let req = Request::post("/rpc/procedure.ping")
        .header(header::CONTENT_TYPE, "application/cbor")
        .header(header::ACCEPT, "application/cbor")
        .header("cratestack-verified-signer", thumbprint)
        .header("x-cose-principal", CLIENT_PRINCIPAL)
        .body(Body::from(ping_payload("n1")))
        .expect("request");

    let answer = send(&router, req).await;

    assert!(!answer.is_sealed());
    assert_eq!(error_code(&answer.body), "permission_denied");
    assert_eq!(procedures.runs(), 0, "a header is not a verified signer");
}
