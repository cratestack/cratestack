//! Session-level refusals (ADR 0021 §1, design §6a.4): an `{id: 0, err}`
//! frame on the control stream carrying the refusal's code, message,
//! `details` and extra members, then the close code its status maps to.

mod support;

use std::collections::BTreeMap;

use cratestack_webtransport::{
    ClientFrame, ServerFrame, WebTransportLimits, WireCodec, close_code,
};
use support::{Client, start};

fn bearer(token: &str) -> BTreeMap<String, String> {
    [("authorization".to_owned(), format!("Bearer {token}"))].into()
}

/// Opens a session and sends `Init` with `headers`; returns the client.
async fn init_with(headers: BTreeMap<String, String>) -> (Client, ServerFrame) {
    let server = start(WebTransportLimits::default());
    let session = server.connect_with(server.request()).await.unwrap();
    let mut client = Client::new(session, WireCodec::Cbor).await;
    let reply = client.init(headers).await;
    // Keep the server alive for as long as the client is used.
    std::mem::forget(server);
    (client, reply)
}

fn err_of(frame: &ServerFrame) -> &cratestack_core::rpc::RpcErrorBody {
    assert_eq!(
        frame.id, 0,
        "a session refusal answers Init, id 0: {frame:?}"
    );
    frame.err.as_ref().expect("an err frame")
}

#[tokio::test]
async fn a_401_refusal_sends_its_members_then_closes_with_4401() {
    let (client, reply) = init_with(bearer("bad")).await;
    let err = err_of(&reply);
    assert_eq!(err.code, "unauthenticated");
    assert_eq!(err.message, "attestation expired");
    assert_eq!(
        err.extra.get("error"),
        Some(&serde_json::json!("attestation_expired"))
    );
    assert!(err.details.is_none());
    assert_eq!(client.close_code().await, close_code::UNAUTHENTICATED);
}

#[tokio::test]
async fn a_403_refusal_sends_details_then_closes_with_4403() {
    let (client, reply) = init_with(bearer("forbidden")).await;
    let err = err_of(&reply);
    assert_eq!(err.code, "permission_denied");
    assert_eq!(err.details, Some(serde_json::json!({ "tenant": "acme" })));
    assert_eq!(
        err.extra.get("error"),
        Some(&serde_json::json!("tenant_mismatch"))
    );
    assert_eq!(client.close_code().await, close_code::FORBIDDEN);
}

#[tokio::test]
async fn a_provider_that_cannot_decide_is_4500_not_a_refusal() {
    let (client, reply) = init_with(bearer("down")).await;
    let err = err_of(&reply);
    assert_eq!(err.code, "unavailable");
    // The same public message REST and RPC give an `Unavailable`.
    assert_eq!(err.message, "key server unreachable");
    assert_eq!(client.close_code().await, close_code::SERVER_ERROR);
}

#[tokio::test]
async fn an_internal_provider_failure_leaks_nothing() {
    let (client, reply) = init_with(bearer("broken")).await;
    let err = err_of(&reply);
    assert_eq!(err.code, "internal");
    assert_eq!(
        err.message, "internal error",
        "operator detail stays out of the frame"
    );
    assert!(err.extra.is_empty() && err.details.is_none());
    assert_eq!(client.close_code().await, close_code::SERVER_ERROR);
}

#[tokio::test]
async fn a_malformed_init_header_is_refused_with_4400() {
    let headers = [("bad header".to_owned(), "x".to_owned())].into();
    let (client, reply) = init_with(headers).await;
    assert_eq!(err_of(&reply).code, "invalid_argument");
    assert_eq!(client.close_code().await, close_code::PROTOCOL);
}

#[tokio::test]
async fn a_refused_reauthentication_is_refused_the_same_way() {
    let (mut client, reply) = init_with(bearer("7")).await;
    assert!(reply.ready.is_some());
    client.send_control(&ClientFrame::init(bearer("bad"))).await;
    let err_frame = client.control_frame().await.expect("an err frame");
    assert_eq!(err_of(&err_frame).code, "unauthenticated");
    assert_eq!(client.close_code().await, close_code::UNAUTHENTICATED);
}

#[tokio::test]
async fn reauthentication_keeps_the_principal_or_is_refused_with_4403() {
    let (mut client, reply) = init_with(bearer("7")).await;
    assert!(reply.ready.is_some());
    client.send_control(&ClientFrame::init(bearer("7"))).await;
    let mut still_open = client
        .subscribe(1, "test.items", serde_json::json!({ "n": 1 }))
        .await;
    assert!(
        still_open.next().await.unwrap().ready.is_some(),
        "same principal: session continues"
    );
    client.send_control(&ClientFrame::init(bearer("8"))).await;
    let err_frame = client.control_frame().await.expect("an err frame");
    let err = err_of(&err_frame);
    assert_eq!(err.code, "permission_denied");
    assert_eq!(err.message, "re-authentication named another principal");
    assert_eq!(client.close_code().await, close_code::FORBIDDEN);
}
