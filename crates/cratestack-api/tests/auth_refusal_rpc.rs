//! cratestack#1208 on the RPC binding, through the real generated
//! `rpc_router()` (`transport rpc`, `db = None`, CBOR): a unary call and a
//! batch, whose envelope is authenticated once before any frame runs.
//! The REST half is `auth_refusal_rest.rs`.

use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode, header::WWW_AUTHENTICATE};
use cratestack::rpc::{RPC_BATCH_PATH, RpcErrorBody, RpcRequest};
use cratestack::{AuthRefusal, CratestackContext, CratestackError, Value, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/rpc_batch_no_database.cstack", db = None);

#[derive(Clone)]
struct RefusingAuthProvider;

impl cratestack::AuthProvider for RefusingAuthProvider {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &cratestack::RequestContext<'_>,
    ) -> Result<CratestackContext, Self::Error> {
        let case = request
            .headers
            .get("x-refusal")
            .and_then(|v| v.to_str().ok());
        Err(match case {
            Some("renew") => AuthRefusal::unauthenticated("attestation expired")
                .with_member("error", Value::String("attestation_expired".into()))
                .with_challenge(r#"Signature error="attestation_expired""#)
                .into_error(),
            Some("revoked") => AuthRefusal::forbidden("device revoked")
                .with_member("error", Value::String("device_revoked".into()))
                .with_details(Value::Map(
                    [("device".to_owned(), Value::String("d-1".into()))].into(),
                ))
                .into_error(),
            _ => CratestackError::Unauthorized("no credential".into()),
        })
    }
}

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> Result<cratestack_schema::procedures::ping::Output, CratestackError> {
        Ok(cratestack_schema::PingReply {
            nonce: args.args.nonce,
        })
    }
}

/// Posts `body` to `path` with the given refusal case; returns the status,
/// the `WWW-Authenticate` values and the decoded RPC error body.
async fn call(path: &str, body: Vec<u8>, case: &str) -> (StatusCode, Vec<String>, RpcErrorBody) {
    let db = cratestack_schema::Cratestack::builder().build();
    let app = cratestack_schema::axum::rpc_router(
        db,
        Procedures,
        (),
        CborCodec,
        RefusingAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let request = Request::post(path)
        .header("content-type", CborCodec::CONTENT_TYPE)
        .header("accept", CborCodec::CONTENT_TYPE)
        .header("x-refusal", case)
        .body(Body::from(body))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let challenges = response
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, challenges, CborCodec.decode(&bytes).unwrap())
}

fn unary_body() -> Vec<u8> {
    let args = cratestack_schema::procedures::ping::Args {
        args: cratestack_schema::PingArgs { nonce: "n".into() },
    };
    CborCodec.encode(&args).unwrap()
}

fn batch_body() -> Vec<u8> {
    let frame = |id: u64| RpcRequest {
        id,
        op: "procedure.ping".into(),
        input: serde_json::json!({ "args": { "nonce": id.to_string() } }),
        idem: None,
    };
    CborCodec.encode(&vec![frame(1), frame(2)]).unwrap()
}

#[tokio::test]
async fn a_unary_refusal_keeps_its_member_and_challenge() {
    let (status, challenges, body) = call("/rpc/procedure.ping", unary_body(), "renew").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(challenges, [r#"Signature error="attestation_expired""#]);
    assert_eq!(body.code, "unauthenticated");
    assert_eq!(body.message, "attestation expired");
    assert_eq!(body.extra["error"], "attestation_expired");
}

#[tokio::test]
async fn a_unary_forbidden_refusal_is_permission_denied_with_details() {
    let (status, _, body) = call("/rpc/procedure.ping", unary_body(), "revoked").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body.code, "permission_denied");
    assert_eq!(body.extra["error"], "device_revoked");
    assert_eq!(body.details, Some(serde_json::json!({ "device": "d-1" })));
}

/// The batch envelope is authenticated once, before any frame: a refusal
/// answers the whole batch, with the same status, members and challenge.
#[tokio::test]
async fn a_batch_refusal_answers_the_whole_batch_the_same_way() {
    let (status, challenges, body) = call(RPC_BATCH_PATH, batch_body(), "renew").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(challenges, [r#"Signature error="attestation_expired""#]);
    assert_eq!(body.code, "unauthenticated");
    assert_eq!(body.extra["error"], "attestation_expired");
}

#[tokio::test]
async fn a_plain_unauthorized_is_unchanged_on_rpc() {
    let (status, challenges, body) = call("/rpc/procedure.ping", unary_body(), "plain").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(challenges.is_empty());
    assert_eq!(body.code, "unauthenticated");
    assert!(body.details.is_none() && body.extra.is_empty());
}
