//! Session admission: path, Origin, Init, authentication, re-authentication
//! and the per-session / per-principal limits.

mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use cratestack_webtransport::{ClientFrame, WebTransportLimits, WireCodec, close_code};
use support::{ALLOWED_ORIGIN, Client, start};

fn bearer(token: &str) -> BTreeMap<String, String> {
    [("authorization".to_owned(), format!("Bearer {token}"))].into()
}

#[tokio::test]
async fn init_headers_authenticate_the_session() {
    let server = start(WebTransportLimits::default());
    let client = server.session(Some("7")).await;
    let mut who = client
        .subscribe(1, "test.whoami", serde_json::Value::Null)
        .await;
    who.next().await.unwrap();
    assert_eq!(who.next().await.unwrap().next, Some(serde_json::json!("7")));
}

#[tokio::test]
async fn connect_headers_authenticate_a_native_client_with_an_empty_init() {
    let server = start(WebTransportLimits::default());
    let request = server
        .request()
        .with_header(http::header::AUTHORIZATION, "Bearer 9".parse().unwrap());
    let mut client =
        Client::new(server.connect_with(request).await.unwrap(), WireCodec::Cbor).await;
    assert!(client.init(BTreeMap::new()).await.ready.is_some());
    let mut who = client
        .subscribe(1, "test.whoami", serde_json::Value::Null)
        .await;
    who.next().await.unwrap();
    assert_eq!(who.next().await.unwrap().next, Some(serde_json::json!("9")));
}

#[tokio::test]
async fn refused_credentials_close_the_session_with_4401() {
    let server = start(WebTransportLimits::default());
    let mut client = Client::new(
        server.connect_with(server.request()).await.unwrap(),
        WireCodec::Cbor,
    )
    .await;
    client.send_control(&ClientFrame::init(bearer("bad"))).await;
    assert_eq!(client.close_code().await, close_code::UNAUTHENTICATED);
}

#[tokio::test]
async fn a_session_without_init_is_closed_with_4401() {
    let limits = WebTransportLimits {
        init_timeout: Duration::from_millis(200),
        ..Default::default()
    };
    let server = start(limits);
    let session = server.connect_with(server.request()).await.unwrap();
    assert_eq!(
        support::close_code(&session).await,
        close_code::UNAUTHENTICATED
    );
}

#[tokio::test]
async fn reauthentication_keeps_the_principal_or_closes_with_4403() {
    let server = start(WebTransportLimits::default());
    let mut client = server.session(Some("7")).await;
    client.send_control(&ClientFrame::init(bearer("7"))).await;
    let mut still_open = client
        .subscribe(1, "test.items", serde_json::json!({ "n": 1 }))
        .await;
    assert!(
        still_open.next().await.unwrap().ready.is_some(),
        "same principal: session continues"
    );
    client.send_control(&ClientFrame::init(bearer("8"))).await;
    assert_eq!(client.close_code().await, close_code::PRINCIPAL_CHANGED);
}

#[tokio::test]
async fn a_request_on_the_control_stream_closes_with_4400() {
    let server = start(WebTransportLimits::default());
    let mut client = server.session(Some("7")).await;
    client
        .send_control(&ClientFrame::request(
            1,
            "test.items",
            serde_json::Value::Null,
        ))
        .await;
    assert_eq!(client.close_code().await, close_code::PROTOCOL);
}

#[tokio::test]
async fn a_cross_origin_page_is_refused_at_connect() {
    let server = start(WebTransportLimits::default());
    let evil = server.request().with_header(
        http::header::ORIGIN,
        "https://evil.example".parse().unwrap(),
    );
    let error = server.connect_with(evil).await.expect_err("refused");
    assert!(error.to_string().contains("403"), "{error}");
    let allowed = server
        .request()
        .with_header(http::header::ORIGIN, ALLOWED_ORIGIN.parse().unwrap());
    assert!(server.connect_with(allowed).await.is_ok());
}

#[tokio::test]
async fn an_unknown_path_is_refused_with_404() {
    let server = start(WebTransportLimits::default());
    let url = server.url.replace("/rpc/wt", "/elsewhere");
    let request = web_transport_quinn::proto::ConnectRequest::new(url.parse::<url::Url>().unwrap());
    let error = server.connect_with(request).await.expect_err("refused");
    assert!(error.to_string().contains("404"), "{error}");
}

#[tokio::test]
async fn subscriptions_past_the_session_limit_get_resource_exhausted() {
    let server = start(WebTransportLimits {
        subscriptions_per_session: 1,
        ..Default::default()
    });
    let client = server.session(Some("7")).await;
    let mut first = client
        .subscribe(1, "test.forever", serde_json::Value::Null)
        .await;
    assert!(first.next().await.unwrap().ready.is_some());
    let mut second = client
        .subscribe(2, "test.items", serde_json::json!({ "n": 1 }))
        .await;
    let frame = second.next().await.unwrap();
    assert_eq!(
        frame.err.as_ref().map(|e| e.code.as_str()),
        Some("resource_exhausted")
    );
    assert!(
        first.next().await.unwrap().next.is_some(),
        "the first subscription is unaffected"
    );
}

#[tokio::test]
async fn sessions_past_the_principal_limit_close_with_4429() {
    let server = start(WebTransportLimits {
        sessions_per_principal: 1,
        ..Default::default()
    });
    let _first = server.session(Some("7")).await;
    let mut second = Client::new(
        server.connect_with(server.request()).await.unwrap(),
        WireCodec::Cbor,
    )
    .await;
    second.send_control(&ClientFrame::init(bearer("7"))).await;
    assert_eq!(second.close_code().await, close_code::TOO_MANY_SESSIONS);
    let _other_principal = server.session(Some("8")).await;
}
