//! ADR 0019 D5 (PR A): a `model` sent as a procedure argument, or held by a
//! `type` that is one, runs the validators of its stored fields, over REST.
//! Only a model's create and update inputs ran them before, so
//! `procedure takeUser(args: User)` accepted `name: "x"` where `POST /users`
//! answered 422. The RPC half is `model_argument_validators_rpc.rs`; both
//! read the one table in `model_argument_support`.
//!
//! Needs no database (see that module): the pool cannot connect.

#![cfg(feature = "codec-json")]

mod model_argument_support;

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{CratestackCodec, CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_json::JsonCodec;
use model_argument_support::{cases, dead_pool};
use tower::ServiceExt;

include_server_schema!(
    "tests/fixtures/model_argument_validators_rest.cstack",
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
    cratestack_schema::axum::router(
        db,
        Procedures,
        (),
        JsonCodec,
        Authenticated,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

async fn post(path: String, body: serde_json::Value) -> (StatusCode, Vec<u8>) {
    let response = router()
        .oneshot(
            Request::post(path)
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(JsonCodec.encode(&body).expect("body encodes")))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, bytes.to_vec())
}

#[tokio::test]
async fn model_arguments_are_validated() {
    for case in cases() {
        let (status, bytes) = post(format!("/$procs/{}", case.procedure), case.body).await;
        match case.rejected {
            None => assert_eq!(status, StatusCode::OK, "{}", case.label),
            Some(message) => {
                assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{}", case.label);
                let error: cratestack::CratestackErrorResponse =
                    JsonCodec.decode(&bytes).expect("error envelope decodes");
                assert_eq!(error.code, "VALIDATION_ERROR", "{}", case.label);
                assert_eq!(error.message, message, "{}", case.label);
                // Path and bound only: the value the client sent is not echoed.
                assert!(!error.message.contains("secret-looking"), "{}", case.label);
            }
        }
    }
}

/// The control the finding was made against: the same value on the model's
/// own create route is refused with the same status and code.
#[tokio::test]
async fn the_create_route_refuses_the_same_value() {
    let (status, bytes) = post(
        "/arg_users".to_owned(),
        serde_json::json!({"id": 1, "name": "x", "nick": null}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let error: cratestack::CratestackErrorResponse = JsonCodec.decode(&bytes).unwrap();
    assert_eq!(error.code, "VALIDATION_ERROR");
    assert_eq!(error.message, "field 'name' length 1 is below minimum 3");
}

/// `ValidateFields` is on the model itself, so a worker holding one validates
/// it without a request.
#[test]
fn a_model_and_a_type_holding_one_implement_the_trait() {
    use cratestack::ValidateFields;
    let user = cratestack_schema::ArgUser {
        id: 1,
        name: "x".into(),
        slug: "abc".into(),
        secret: String::new(),
        nick: None,
    };
    assert!(user.validate().is_err());
    let wrap = cratestack_schema::Wrap { owner: user };
    assert_eq!(
        wrap.validate().unwrap_err().public_message().into_owned(),
        "field 'owner.name' length 1 is below minimum 3"
    );
}
