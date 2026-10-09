//! A caller that does its own HTTP seals a call with
//! [`ClientEnvelope::seal_call`] and opens the answer with
//! [`PendingResponse::open`] (cratestack#1168). The server here is a stub
//! with a real server envelope; the HTTP client is plain `reqwest`.

#![cfg(feature = "cose")]

use std::borrow::Cow;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, HeaderValue};
use axum::routing::post;
use cratestack_client_rust::cose::{
    CoseAlg, CoseEnvelope, CoseMode, HmacSigner, StaticVerifierResolver, request_digest,
};
use cratestack_client_rust::{
    ClientEnvelope, ClientError, EnvelopeError, RouteRef, SealCall, ensure_crypto_provider,
};
use cratestack_core::{
    Binding, BoundHeaders, CONTRACT_HEADER, ContractSelector, InMemoryNonceStore,
    PAYLOAD_ACCEPT_HEADER, PAYLOAD_TYPE_HEADER, PathParams, ResponseBinding,
};

const FORM: &str = "application/x-www-form-urlencoded";
const JSON: &str = "application/json";
const CBOR: &str = "application/cbor";
const CONTRACT: [u8; 32] = [0x55; 32];
const MAC0: &str = "application/cose; cose-type=\"cose-mac0\"";

fn signer() -> HmacSigner {
    HmacSigner::new(CoseAlg::Hmac256_64, vec![3; 32]).unwrap()
}

fn resolver() -> Arc<StaticVerifierResolver> {
    Arc::new(StaticVerifierResolver::new().with_key(signer().verify_key()))
}

fn envelope() -> ClientEnvelope {
    let cose = CoseEnvelope::client(CoseMode::Mac0, Arc::new(signer()), resolver())
        .build()
        .unwrap();
    ClientEnvelope::new(cose, "payments").unwrap()
}

fn server() -> CoseEnvelope {
    CoseEnvelope::server(
        CoseMode::Mac0,
        Arc::new(signer()),
        resolver(),
        Arc::new(InMemoryNonceStore::new()),
    )
    .build()
    .unwrap()
}

fn binding(payload_type: &'static str, idempotency_key: Option<&'static str>) -> Binding<'static> {
    Binding {
        audience: Cow::Borrowed("payments"),
        method: Cow::Borrowed("POST"),
        route: Cow::Borrowed("/charges/{id}"),
        path_params: PathParams::Borrowed(&["ch_1"]),
        query: Some(Cow::Borrowed("expand=customer")),
        contract_sha: CONTRACT,
        payload_media_type: Cow::Borrowed(payload_type),
        bound_headers: BoundHeaders {
            idempotency_key: idempotency_key.map(Cow::Borrowed),
            if_match: None,
        },
        response: None,
    }
}

fn call<'a>(route: &'a RouteRef<'a>) -> SealCall<'a> {
    SealCall::new("POST", *route, CONTRACT)
        .query(Some("expand=customer"))
        .payload(b"amount=1500", FORM)
        .accept(JSON)
        .idempotency_key("idem-1")
}

/// A stub that opens the request under the binding a form call has, and
/// answers `answer` sealed under `answer_type`.
async fn stub(answer_type: &'static str, says: &'static str) -> std::net::SocketAddr {
    ensure_crypto_provider();
    let handler = move |headers: HeaderMap, body: Bytes| async move {
        let opened = server()
            .open_request(body.clone(), &binding(FORM, Some("idem-1")))
            .await
            .expect("the request opens under the binding the call named");
        assert_eq!(opened.payload.as_ref(), b"amount=1500");
        assert_eq!(headers[PAYLOAD_TYPE_HEADER], FORM);
        assert_eq!(headers[PAYLOAD_ACCEPT_HEADER], JSON);
        assert_eq!(headers["idempotency-key"], "idem-1");
        assert_eq!(headers[CONTENT_TYPE], MAC0);
        assert_eq!(headers["accept"], MAC0);
        assert_eq!(
            headers[CONTRACT_HEADER].to_str().unwrap(),
            ContractSelector::of(&CONTRACT).to_header_value()
        );
        let bind = Binding {
            response: Some(ResponseBinding {
                request: request_digest(&body),
                status: 200,
            }),
            ..binding(answer_type, Some("idem-1"))
        };
        let sealed = server()
            .seal_response(b"{\"id\":\"ch_1\"}", &bind)
            .await
            .unwrap();
        let mut out = HeaderMap::new();
        out.insert(CONTENT_TYPE, HeaderValue::from_static(MAC0));
        out.insert(PAYLOAD_TYPE_HEADER, HeaderValue::from_static(says));
        (out, sealed)
    };
    let router = Router::new().route("/charges/ch_1", post(handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    addr
}

async fn send(
    addr: std::net::SocketAddr,
    sealed: cratestack_client_rust::SealedCall,
) -> (
    reqwest::StatusCode,
    HeaderMap,
    Bytes,
    cratestack_client_rust::PendingResponse,
) {
    let mut request = reqwest::Client::new()
        .post(format!("http://{addr}/charges/ch_1?expand=customer"))
        .body(sealed.body.clone());
    for (name, value) in &sealed.headers {
        request = request.header(name, value);
    }
    let response = request.send().await.unwrap();
    let (status, headers) = (response.status(), response.headers().clone());
    (
        status,
        headers,
        response.bytes().await.unwrap(),
        sealed.pending,
    )
}

#[tokio::test]
async fn a_call_sealed_here_and_sent_by_plain_reqwest_is_accepted_and_its_answer_opens() {
    let addr = stub(JSON, JSON).await;
    let route = RouteRef::new("/charges/{id}", &["ch_1"]);
    let sealed = envelope().seal_call(call(&route)).await.expect("sealed");
    let (status, headers, body, pending) = send(addr, sealed).await;
    assert_eq!(status, 200);
    let opened = pending
        .open(status.as_u16(), &headers, body)
        .await
        .expect("opens");
    assert_eq!(opened.payload_type, JSON);
    assert_eq!(opened.body.as_ref(), b"{\"id\":\"ch_1\"}");
}

#[tokio::test]
async fn an_answer_in_a_type_that_was_not_asked_for_is_refused_unread() {
    let addr = stub(CBOR, CBOR).await;
    let route = RouteRef::new("/charges/{id}", &["ch_1"]);
    let sealed = envelope().seal_call(call(&route)).await.expect("sealed");
    let (status, headers, body, pending) = send(addr, sealed).await;
    let error = pending
        .open(status.as_u16(), &headers, body)
        .await
        .expect_err("refused");
    assert!(
        matches!(error, ClientError::Envelope(EnvelopeError::UnexpectedPayloadType { ref got }) if got == CBOR),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_answer_for_another_status_or_request_does_not_verify() {
    let addr = stub(JSON, JSON).await;
    let route = RouteRef::new("/charges/{id}", &["ch_1"]);
    let sealed = envelope().seal_call(call(&route)).await.expect("sealed");
    let (_, headers, body, pending) = send(addr, sealed).await;
    // Sealed for 200: opening it as a 500 fails.
    let error = pending
        .open(500, &headers, body)
        .await
        .expect_err("status is bound");
    assert!(
        matches!(error, ClientError::Envelope(EnvelopeError::Unverified)),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_unsealed_answer_is_unsigned_whatever_its_status() {
    let route = RouteRef::new("/charges/{id}", &["ch_1"]);
    let sealed = envelope().seal_call(call(&route)).await.expect("sealed");
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(JSON));
    let error = sealed
        .pending
        .open(200, &headers, Bytes::from_static(b"{\"id\":\"forged\"}"))
        .await
        .expect_err("not sealed");
    assert!(
        matches!(
            error,
            ClientError::Envelope(EnvelopeError::Unsigned { status: 200 })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_call_that_cannot_be_sealed_is_refused_before_anything_is_signed() {
    let route = RouteRef::new("/charges/{id}", &["ch_1"]);
    let batch = RouteRef::rpc("batch");
    let refused: Vec<SealCall<'_>> = vec![
        // Not a payload type.
        call(&route).payload(b"x", "Application/JSON"),
        call(&route).payload(b"x", "application/cose"),
        call(&route).payload(b"x", "text/event-stream"),
        // Not an accept list.
        call(&route).accept("application/json,application/cbor"),
        call(&route).accept("*/*"),
        // Bound headers must survive a hop untouched.
        call(&route).idempotency_key(" idem-1"),
        call(&route).if_match("etag "),
        // The batch is CBOR both ways.
        SealCall::new("POST", batch, CONTRACT).payload(b"x", JSON),
        SealCall::new("POST", batch, CONTRACT).accept(JSON),
    ];
    for (index, refused) in refused.into_iter().enumerate() {
        let error = envelope().seal_call(refused).await.err().expect("refused");
        assert!(
            matches!(error, ClientError::BadInput(_)),
            "#{index}: {error:?}"
        );
    }
    // The defaults are CBOR both ways, and a batch accepts them.
    envelope()
        .seal_call(SealCall::new("POST", batch, CONTRACT).payload(b"x", CBOR))
        .await
        .expect("a CBOR batch");
}

#[tokio::test]
async fn cbor_defaults_add_no_selector_headers() {
    let route = RouteRef::new("/charges/{id}", &["ch_1"]);
    let sealed = envelope()
        .seal_call(SealCall::new("POST", route, CONTRACT).payload(b"\xa0", CBOR))
        .await
        .expect("sealed");
    let names: Vec<String> = sealed
        .headers
        .iter()
        .map(|(n, _)| n.as_str().to_owned())
        .collect();
    assert!(
        !names
            .iter()
            .any(|n| n.eq_ignore_ascii_case(PAYLOAD_TYPE_HEADER)),
        "{names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|n| n.eq_ignore_ascii_case(PAYLOAD_ACCEPT_HEADER)),
        "{names:?}"
    );
    assert!(names.iter().any(|n| n == "content-type") && names.iter().any(|n| n == "accept"));
    assert!(
        names
            .iter()
            .any(|n| n.eq_ignore_ascii_case(CONTRACT_HEADER))
    );
}
