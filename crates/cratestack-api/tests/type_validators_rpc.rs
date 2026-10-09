//! ADR 0019 D5 (PR A): a validator on a `type` field runs on a procedure
//! argument over RPC, unary and batch, JSON and CBOR. The REST half is
//! `type_validators_rest.rs`; both read the one table in
//! `type_validators_support`. Nothing here differs from REST in what it
//! asserts: the check is in `authorize_with_db`, below both dispatchers, so
//! the same table must give the same answers.

mod type_validators_support;

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::rpc::{RPC_BATCH_PATH, RpcErrorBody, RpcRequest, RpcResponseFrame};
use cratestack::{CratestackCodec, CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;
use type_validators_support::cases;

include_server_schema!("tests/fixtures/type_validators_rpc.cstack", db = None);

use cratestack_schema::procedures as p;

#[derive(Clone, Default)]
struct Procedures;

macro_rules! reply {
    ($name:ident) => {
        fn $name(
            &self,
            _db: &cratestack_schema::Cratestack,
            _ctx: &CratestackContext,
            _args: p::$name::Args,
            _authorized: p::$name::Authorized,
        ) -> impl core::future::Future<Output = Result<p::$name::Output, CratestackError>> + Send {
            async { Ok(cratestack_schema::Reply { ok: true }) }
        }
    };
}

impl p::ProcedureRegistry for Procedures {
    reply!(greet);
    reply!(open_account);
    reply!(relabel);
    reply!(plain);
    reply!(walk);
    reply!(meet);
    reply!(deep);
}

#[derive(Clone)]
struct Authenticated;

impl cratestack::AuthProvider for Authenticated {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

fn router<C: cratestack::HttpTransport>(codec: C) -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::rpc_router(
        db,
        Procedures,
        (),
        codec,
        Authenticated,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

async fn post(
    router: cratestack::axum::Router,
    content_type: &'static str,
    path: String,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let response = router
        .oneshot(
            Request::post(path)
                .header("content-type", content_type)
                .header("accept", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, bytes.to_vec())
}

/// `POST /rpc/procedure.<name>`: a rejected case is the 422 envelope with the
/// RPC code `invalid_argument`, the message REST gives under
/// `VALIDATION_ERROR`.
macro_rules! run_unary {
    ($codec:ident) => {
        for case in cases() {
            let body = $codec.encode(&case.body).expect("body encodes");
            let path = format!("/rpc/procedure.{}", case.procedure);
            let (status, bytes) = post(router($codec), $codec::CONTENT_TYPE, path, body).await;
            match case.rejected {
                None => assert_eq!(status, StatusCode::OK, "{}", case.label),
                Some(message) => {
                    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{}", case.label);
                    let error: RpcErrorBody = $codec.decode(&bytes).expect("error decodes");
                    assert_eq!(error.code, "invalid_argument", "{}", case.label);
                    assert_eq!(error.message, message, "{}", case.label);
                }
            }
        }
    };
}

/// `POST /rpc/batch`: the whole table in one request. A rejected frame is an
/// error frame; its neighbours are unaffected and the batch itself is 200.
macro_rules! run_batch {
    ($codec:ident) => {
        let table = cases();
        let frames: Vec<RpcRequest> = table
            .iter()
            .enumerate()
            .map(|(id, case)| RpcRequest {
                id: id as u64,
                op: format!("procedure.{}", case.procedure),
                input: case.body.clone(),
                idem: None,
            })
            .collect();
        let body = $codec.encode(&frames).expect("batch encodes");
        let (status, bytes) = post(
            router($codec),
            $codec::CONTENT_TYPE,
            RPC_BATCH_PATH.to_owned(),
            body,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let answers: Vec<RpcResponseFrame> = $codec.decode(&bytes).expect("batch decodes");
        assert_eq!(answers.len(), table.len());
        for (case, answer) in table.iter().zip(&answers) {
            match case.rejected {
                None => assert!(answer.error.is_none(), "{}: {answer:?}", case.label),
                Some(message) => {
                    let error = answer.error.as_ref().expect(case.label);
                    assert_eq!(error.code, "invalid_argument", "{}", case.label);
                    assert_eq!(error.message, message, "{}", case.label);
                }
            }
        }
    };
}

#[tokio::test]
async fn json_unary_arguments_are_validated() {
    run_unary!(JsonCodec);
}

#[tokio::test]
async fn cbor_unary_arguments_are_validated() {
    run_unary!(CborCodec);
}

#[tokio::test]
async fn json_batch_arguments_are_validated() {
    run_batch!(JsonCodec);
}

#[tokio::test]
async fn cbor_batch_arguments_are_validated() {
    run_batch!(CborCodec);
}
