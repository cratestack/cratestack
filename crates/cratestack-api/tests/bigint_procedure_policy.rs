//! ADR 0019 (PR B, risk 2): a `BigInt` procedure argument reaches a policy as
//! a real value, so `!=` and `==` against it decide correctly.
//!
//! The generator used to fall to `Value::Null` for any scalar it had no arm
//! for. That compiled, and it made `owner != 7` true for every caller and
//! `a == b` true for every pair: the policy the author wrote to refuse a
//! request let it through, with nothing pointing at the arm. In-process
//! against `db = None`, REST, over both codecs: a `BigInt` is a string on
//! each, so the argument arrives the way a real client sends it.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{
    BigInt, CratestackCodec, CratestackContext, CratestackError, ProcedureArgs, Value,
    include_server_schema,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use serde_json::json;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/bigint_procedure_policy.cstack", db = None);

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
            async { Ok(cratestack_schema::Receipt { ok: true }) }
        }
    };
}

impl p::ProcedureRegistry for Procedures {
    reply!(not_seven);
    reply!(deny_seven);
    reply!(not_beyond_two_pow53);
    reply!(same_owner);
    reply!(other_owner);
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
            Value::Int(1),
        )])))
    }
}

fn router<C: cratestack::HttpTransport>(codec: C) -> cratestack::axum::Router {
    cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        codec,
        Authenticated,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

async fn status<C: cratestack::HttpTransport + CratestackCodec>(
    codec: C,
    content_type: &'static str,
    procedure: &str,
    body: serde_json::Value,
) -> StatusCode {
    let response = router(codec.clone())
        .oneshot(
            Request::post(format!("/$procs/{procedure}"))
                .header("content-type", content_type)
                .header("accept", content_type)
                .body(Body::from(codec.encode(&body).expect("body encodes")))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let _ = to_bytes(response.into_body(), usize::MAX).await;
    status
}

const TWO_POW_53_PLUS_1: &str = "9007199254740993";
const TWO_POW_53: &str = "9007199254740992";

/// `(procedure, body, expected status)`; one table for both codecs.
fn cases() -> Vec<(&'static str, serde_json::Value, StatusCode)> {
    vec![
        ("notSeven", json!({ "owner": "7" }), StatusCode::FORBIDDEN),
        ("notSeven", json!({ "owner": "8" }), StatusCode::OK),
        ("notSeven", json!({ "owner": "-7" }), StatusCode::OK),
        ("denySeven", json!({ "owner": "7" }), StatusCode::FORBIDDEN),
        ("denySeven", json!({ "owner": "8" }), StatusCode::OK),
        (
            "notBeyondTwoPow53",
            json!({ "owner": TWO_POW_53_PLUS_1 }),
            StatusCode::FORBIDDEN,
        ),
        (
            "notBeyondTwoPow53",
            json!({ "owner": TWO_POW_53 }),
            StatusCode::OK,
        ),
        (
            "sameOwner",
            json!({ "a": "1", "b": "2" }),
            StatusCode::FORBIDDEN,
        ),
        ("sameOwner", json!({ "a": "5", "b": "5" }), StatusCode::OK),
        (
            "sameOwner",
            json!({ "a": TWO_POW_53_PLUS_1, "b": TWO_POW_53 }),
            StatusCode::FORBIDDEN,
        ),
        ("otherOwner", json!({ "a": "1", "b": "2" }), StatusCode::OK),
        (
            "otherOwner",
            json!({ "a": "5", "b": "5" }),
            StatusCode::FORBIDDEN,
        ),
        // The wire contract, end to end: a JSON number is not a `BigInt`.
        ("notSeven", json!({ "owner": 8 }), StatusCode::BAD_REQUEST),
    ]
}

#[tokio::test]
async fn json_decides_a_bigint_argument_correctly() {
    for (procedure, body, expected) in cases() {
        let got = status(JsonCodec, JsonCodec::CONTENT_TYPE, procedure, body.clone()).await;
        assert_eq!(got, expected, "JSON {procedure} {body}");
    }
}

#[tokio::test]
async fn cbor_decides_a_bigint_argument_correctly() {
    for (procedure, body, expected) in cases() {
        let got = status(CborCodec, CborCodec::CONTENT_TYPE, procedure, body.clone()).await;
        assert_eq!(got, expected, "CBOR {procedure} {body}");
    }
}

/// What the policy evaluator reads: a `BigInt` argument is a `Value::Int`,
/// never `Null`, and an argument the procedure does not have is `None`.
#[test]
fn the_generated_args_report_a_bigint_as_a_value() {
    let args = p::same_owner::Args {
        a: BigInt::new(i64::MAX),
        b: BigInt::new(i64::MIN),
    };
    assert_eq!(args.procedure_arg_value("a"), Some(Value::Int(i64::MAX)));
    assert_eq!(args.procedure_arg_value("b"), Some(Value::Int(i64::MIN)));
    assert_eq!(args.procedure_arg_value("c"), None);
}
