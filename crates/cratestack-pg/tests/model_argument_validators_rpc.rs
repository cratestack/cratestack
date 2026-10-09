//! ADR 0019 D5 (PR A): a `model` sent as a procedure argument runs the
//! validators of its stored fields, over RPC, unary and batch. The REST half
//! is `model_argument_validators_rest.rs`; both read the one table in
//! `model_argument_support`. The check is in the generated procedure helpers,
//! below both dispatchers, so the same table must give the same answers.
//!
//! Needs no database (see that module): the pool cannot connect.

#![cfg(feature = "codec-json")]

mod model_argument_support;

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::rpc::{RPC_BATCH_PATH, RpcErrorBody, RpcRequest, RpcResponseFrame};
use cratestack::{CratestackCodec, CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_json::JsonCodec;
use model_argument_support::{cases, dead_pool};
use tower::ServiceExt;

include_server_schema!(
    "tests/fixtures/model_argument_validators_rpc.cstack",
    db = Postgres
);

use cratestack_schema::procedures as p;

#[derive(Clone, Default)]
struct Procedures;

macro_rules! reply {
    ($name:ident, $db:ty) => {
        fn $name(
            &self,
            _db: &$db,
            _ctx: &CratestackContext,
            _args: p::$name::Args,
            _authorized: p::$name::Authorized,
        ) -> impl core::future::Future<Output = Result<p::$name::Output, CratestackError>> + Send {
            async { Ok(cratestack_schema::Reply { ok: true }) }
        }
    };
}

impl p::ProcedureRegistry for Procedures {
    reply!(take_user, cratestack_schema::Cratestack);
    reply!(take_wrap, cratestack_schema::Cratestack);
    reply!(take_many, cratestack_schema::Cratestack);
    reply!(take_isolated, cratestack_schema::IsolatedCratestack);
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

fn router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder(dead_pool()).build();
    cratestack_schema::axum::rpc_router(
        db,
        Procedures,
        (),
        JsonCodec,
        Authenticated,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

async fn post(path: String, body: Vec<u8>) -> (StatusCode, Vec<u8>) {
    let response = router()
        .oneshot(
            Request::post(path)
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, bytes.to_vec())
}

#[tokio::test]
async fn unary_model_arguments_are_validated() {
    for case in cases() {
        let body = JsonCodec.encode(&case.body).expect("body encodes");
        let (status, bytes) = post(format!("/rpc/procedure.{}", case.procedure), body).await;
        match case.rejected {
            None => assert_eq!(status, StatusCode::OK, "{}", case.label),
            Some(message) => {
                assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{}", case.label);
                let error: RpcErrorBody = JsonCodec.decode(&bytes).expect("error decodes");
                assert_eq!(error.code, "invalid_argument", "{}", case.label);
                assert_eq!(error.message, message, "{}", case.label);
            }
        }
    }
}

/// The whole table in one `/rpc/batch` request: a rejected frame is an error
/// frame, its neighbours are unaffected and the batch itself is 200.
#[tokio::test]
async fn batch_model_arguments_are_validated() {
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
    let body = JsonCodec.encode(&frames).expect("batch encodes");
    let (status, bytes) = post(RPC_BATCH_PATH.to_owned(), body).await;
    assert_eq!(status, StatusCode::OK);
    let answers: Vec<RpcResponseFrame> = JsonCodec.decode(&bytes).expect("batch decodes");
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
}

#[tokio::test]
async fn the_create_op_refuses_the_same_value() {
    let body = JsonCodec
        .encode(&serde_json::json!({"id": 1, "name": "x", "nick": null}))
        .expect("body encodes");
    let (status, bytes) = post("/rpc/model.ArgUser.create".to_owned(), body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let error: RpcErrorBody = JsonCodec.decode(&bytes).expect("error decodes");
    assert_eq!(error.code, "invalid_argument");
    assert_eq!(error.message, "field 'name' length 1 is below minimum 3");
}
