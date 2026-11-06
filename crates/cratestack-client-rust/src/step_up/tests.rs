//! cratestack#1244: a step-up refusal reads the same on REST, an RPC unary
//! answer and a batch frame; anything else is not one; a mistyped field
//! never makes reading fail; the retry helper retries at most once.

use std::cell::Cell;

use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;
use cratestack_core::rpc::RpcErrorBody;
use serde_json::{Value, json};

use super::{StepUpRequired, retry_after_step_up};
use crate::client::decode::remote_error_from_response;
use crate::error::ClientError;
use crate::remote_refusal::Challenges;
use crate::rpc::error::{RpcClientError, RpcRemoteError, decode_rpc_unary_response};
use crate::runtime::wire::{RuntimeHeader, RuntimeResponseWire};

fn details() -> Value {
    json!({ "max_age": 300, "acr_values": ["aal2", "aal3"], "unmet": ["max_age", "acr"] })
}

fn envelope(code: &str, error: &str, details: Value) -> Value {
    json!({
        "code": code,
        "message": "step-up authentication required",
        "details": details,
        "error": error,
    })
}

fn answer(status: u16, body: &Value) -> RuntimeResponseWire {
    RuntimeResponseWire {
        status_code: status,
        headers: vec![RuntimeHeader {
            name: "content-type".to_owned(),
            value: CborCodec::CONTENT_TYPE.to_owned(),
        }],
        body: CborCodec.encode(body).expect("encode"),
    }
}

fn rest(status: u16, body: &Value) -> ClientError {
    remote_error_from_response(&CborCodec, &answer(status, body))
}

fn rpc(body: &Value) -> RpcClientError {
    decode_rpc_unary_response::<_, Value>(&CborCodec, &answer(401, body)).expect_err("a refusal")
}

fn expected() -> StepUpRequired {
    StepUpRequired {
        max_age: Some(300),
        acr_values: vec!["aal2".into(), "aal3".into()],
        amr: Vec::new(),
        unmet: vec!["max_age".into(), "acr".into()],
    }
}

const STEP_UP: &str = "insufficient_user_authentication";

#[test]
fn a_rest_step_up_is_read_from_its_details() {
    let error = rest(401, &envelope("UNAUTHORIZED", STEP_UP, details()));
    assert_eq!(error.step_up(), Some(expected()));
}

#[test]
fn an_rpc_step_up_is_read_on_a_unary_answer_and_a_batch_frame() {
    let error = rpc(&envelope("unauthenticated", STEP_UP, details()));
    assert_eq!(error.step_up(), Some(expected()));

    let mut body = RpcErrorBody {
        code: "unauthenticated".to_owned(),
        message: "step-up authentication required".to_owned(),
        details: Some(json!({ "amr": ["hwk"], "unmet": ["amr"] })),
        extra: Default::default(),
    };
    body.extra.insert("error".to_owned(), json!(STEP_UP));
    let frame = RpcRemoteError {
        status: reqwest::StatusCode::UNAUTHORIZED,
        body,
        challenges: Challenges::default(),
    };
    let step_up = frame.step_up().expect("a step-up");
    assert_eq!(step_up.amr, ["hwk"]);
    assert_eq!(step_up.max_age, None);
    assert!(step_up.acr_values.is_empty());
}

#[test]
fn a_plain_401_or_another_refusal_is_not_a_step_up() {
    let plain = json!({ "code": "UNAUTHORIZED", "message": "no credential" });
    assert_eq!(rest(401, &plain).step_up(), None);
    let renew = envelope("UNAUTHORIZED", "invalid_token", details());
    assert_eq!(rest(401, &renew).step_up(), None);
    // The right member on the wrong status, or the wrong RPC code.
    assert_eq!(
        rest(403, &envelope("FORBIDDEN", STEP_UP, details())).step_up(),
        None
    );
    assert_eq!(
        rpc(&envelope("permission_denied", STEP_UP, details())).step_up(),
        None
    );
    assert_eq!(ClientError::State("x".into()).step_up(), None);
    assert_eq!(RpcClientError::BadInput("x".into()).step_up(), None);
}

#[test]
fn mistyped_details_read_as_absent() {
    let mistyped =
        json!({ "max_age": "300", "acr_values": "aal2", "amr": [1, "pwd"], "unmet": {} });
    for details in [mistyped, json!("nope"), Value::Null] {
        let step_up = rest(401, &envelope("UNAUTHORIZED", STEP_UP, details.clone()))
            .step_up()
            .expect("still a step-up");
        assert_eq!(step_up.max_age, None);
        assert!(step_up.acr_values.is_empty() && step_up.unmet.is_empty());
        let rpc_step_up = rpc(&envelope("unauthenticated", STEP_UP, details))
            .step_up()
            .expect("still a step-up");
        assert_eq!(rpc_step_up, step_up);
    }
    let negative = json!({ "max_age": -1, "amr": [1, "pwd"] });
    let step_up = rest(401, &envelope("UNAUTHORIZED", STEP_UP, negative))
        .step_up()
        .unwrap();
    assert_eq!(
        (step_up.max_age, step_up.amr),
        (None, vec!["pwd".to_owned()])
    );
}

fn refused() -> ClientError {
    rest(401, &envelope("UNAUTHORIZED", STEP_UP, details()))
}

#[tokio::test]
async fn the_call_is_retried_once_after_a_reproof() {
    let calls = Cell::new(0);
    let result = retry_after_step_up(
        || {
            calls.set(calls.get() + 1);
            let attempt = calls.get();
            async move {
                if attempt == 1 {
                    Err(refused())
                } else {
                    Ok(attempt)
                }
            }
        },
        |step_up| async move { step_up == expected() },
    )
    .await;
    assert_eq!(result.expect("the retry succeeded"), 2);
}

#[tokio::test]
async fn a_second_step_up_is_returned_not_retried() {
    let (calls, reproofs) = (Cell::new(0), Cell::new(0));
    let result: Result<(), _> = retry_after_step_up(
        || {
            calls.set(calls.get() + 1);
            async { Err(refused()) }
        },
        |_| {
            reproofs.set(reproofs.get() + 1);
            async { true }
        },
    )
    .await;
    assert!(result.expect_err("refused twice").step_up().is_some());
    assert_eq!((calls.get(), reproofs.get()), (2, 1));
}

#[tokio::test]
async fn a_declined_reproof_or_another_error_is_returned_as_is() {
    let calls = Cell::new(0);
    let declined: Result<(), _> = retry_after_step_up(
        || {
            calls.set(calls.get() + 1);
            async { Err(refused()) }
        },
        |_| async { false },
    )
    .await;
    assert!(declined.expect_err("declined").step_up().is_some());
    assert_eq!(calls.get(), 1);

    let other: Result<(), RpcClientError> = retry_after_step_up(
        || async { Err(RpcClientError::BadInput("x".into())) },
        |_| async { panic!("a non-step-up error never asks for a re-proof") },
    )
    .await;
    assert!(matches!(other, Err(RpcClientError::BadInput(_))));
}
