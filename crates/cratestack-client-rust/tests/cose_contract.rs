//! A sealed call binds the digest of *its own op* and names it in the
//! unbound `Cratestack-Contract` header (binding version 2, cratestack#1123).
//!
//! The server here is a recording stub: it keeps what it was sent, and the
//! test opens the captured message with a real server envelope under the
//! binding it expects, so the digest asserted is the one in the signed AAD.

#![cfg(feature = "cose")]

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use cratestack_client_rust::cose::{
    CoseAlg, CoseEnvelope, CoseMode, HmacSigner, StaticVerifierResolver,
};
use cratestack_client_rust::{
    CborCodec, ClientConfig, ClientEnvelope, ClientError, CratestackClient, EnvelopeError,
    RouteRef, ensure_crypto_provider,
};
use cratestack_core::{
    Binding, BoundHeaders, CONTRACT_HEADER, ContractSelector, InMemoryNonceStore, OpContracts,
    PathParams,
};
use cratestack_core::{CONTRACT_UNSUPPORTED_CODE, CONTRACT_UNSUPPORTED_REST_CODE, CratestackCodec};
use url::Url;

const GET_WIDGETS: [u8; 32] = [0x11; 32];
const POST_WIDGETS: [u8; 32] = [0x22; 32];
const TABLE: OpContracts = &[
    ("GET /widgets", GET_WIDGETS),
    ("POST /widgets", POST_WIDGETS),
];

type Seen = Arc<Mutex<Vec<(HeaderMap, Bytes)>>>;

fn signer() -> HmacSigner {
    HmacSigner::new(CoseAlg::Hmac256_64, vec![9; 32]).unwrap()
}

fn client_envelope() -> CoseEnvelope {
    let signer = signer();
    CoseEnvelope::client(
        CoseMode::Mac0,
        Arc::new(signer.clone()),
        Arc::new(StaticVerifierResolver::new().with_key(signer.verify_key())),
    )
    .build()
    .unwrap()
}

fn server_envelope() -> CoseEnvelope {
    let signer = signer();
    CoseEnvelope::server(
        CoseMode::Mac0,
        Arc::new(signer.clone()),
        Arc::new(StaticVerifierResolver::new().with_key(signer.verify_key())),
        Arc::new(InMemoryNonceStore::new()),
    )
    .build()
    .unwrap()
}

/// A stub answering `status` (unsigned, with `body`), and what it saw.
async fn stub(status: StatusCode, body: &'static str) -> (std::net::SocketAddr, Seen) {
    stub_with(status, "text/plain", body.as_bytes().to_vec()).await
}

/// [`stub`] with an explicit `Content-Type` and raw body bytes.
async fn stub_with(
    status: StatusCode,
    content_type: &'static str,
    body: Vec<u8>,
) -> (std::net::SocketAddr, Seen) {
    ensure_crypto_provider();
    let seen: Seen = Arc::default();
    let recorded = seen.clone();
    let router = Router::new().route(
        "/widgets",
        get(move |headers: HeaderMap, bytes: Bytes| {
            let recorded = recorded.clone();
            async move {
                recorded.lock().unwrap().push((headers, bytes));
                (status, [(CONTENT_TYPE, content_type)], body)
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (addr, seen)
}

fn client(addr: std::net::SocketAddr) -> CratestackClient {
    CratestackClient::new(
        ClientConfig::new(Url::parse(&format!("http://{addr}")).unwrap()),
        CborCodec,
    )
    .with_envelope(ClientEnvelope::new(client_envelope(), "payments").unwrap())
    .unwrap()
}

fn binding(contract: [u8; 32]) -> Binding<'static> {
    Binding {
        audience: Cow::Borrowed("payments"),
        method: Cow::Borrowed("GET"),
        route: Cow::Borrowed("/widgets"),
        path_params: PathParams::EMPTY,
        query: None,
        contract_sha: contract,
        payload_media_type: Cow::Borrowed("application/cbor"),
        bound_headers: BoundHeaders::NONE,
        response: None,
    }
}

async fn get_widgets(client: &CratestackClient) -> Result<serde_json::Value, ClientError> {
    client
        .at(RouteRef::new("/widgets", &[]))
        .get::<serde_json::Value>("/widgets", &[], &[])
        .await
}

#[tokio::test]
async fn a_call_binds_its_ops_digest_and_selects_it_in_the_header() {
    let (addr, seen) = stub(StatusCode::OK, "plain").await;
    let client = client(addr).with_contracts(TABLE);
    let _ = get_widgets(&client).await;
    let (headers, sealed) = seen.lock().unwrap().remove(0);
    let selector = ContractSelector::of(&GET_WIDGETS).to_header_value();
    assert_eq!(headers[CONTRACT_HEADER].to_str().unwrap(), selector);
    // The signed AAD carries GET /widgets's digest, not POST /widgets's.
    server_envelope()
        .open_request(sealed.clone(), &binding(POST_WIDGETS))
        .await
        .expect_err("another op's digest does not open it");
    server_envelope()
        .open_request(sealed, &binding(GET_WIDGETS))
        .await
        .expect("its own op's digest does");
}

#[tokio::test]
async fn a_pinned_digest_binds_every_call() {
    let (addr, seen) = stub(StatusCode::OK, "plain").await;
    let client = client(addr).with_contract_sha(POST_WIDGETS);
    let _ = get_widgets(&client).await;
    let (_, sealed) = seen.lock().unwrap().remove(0);
    server_envelope()
        .open_request(sealed, &binding(POST_WIDGETS))
        .await
        .expect("the pinned digest");
}

#[tokio::test]
async fn an_op_the_table_lacks_is_bad_input_and_nothing_is_sent() {
    let (addr, seen) = stub(StatusCode::OK, "plain").await;
    let client = client(addr).with_contracts(&[("POST /widgets", POST_WIDGETS)]);
    let error = get_widgets(&client).await.expect_err("GET has no row");
    assert!(
        matches!(&error, ClientError::BadInput(message) if message.contains("GET /widgets")),
        "{error:?}"
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "fail closed, before sending"
    );
}

#[tokio::test]
async fn a_client_with_no_digests_is_bad_input() {
    let (addr, seen) = stub(StatusCode::OK, "plain").await;
    let error = get_widgets(&client(addr)).await.expect_err("no digests");
    assert!(matches!(error, ClientError::BadInput(_)), "{error:?}");
    assert!(seen.lock().unwrap().is_empty());
}

/// The layer's own unsigned `426` body, in the codec the client asked for.
fn refusal_body(code: &str) -> Vec<u8> {
    CborCodec
        .encode(&serde_json::json!({ "code": code, "message": "update the client" }))
        .unwrap()
}

async fn answer_426(code: &str) -> ClientError {
    let (addr, _) = stub_with(
        StatusCode::UPGRADE_REQUIRED,
        "application/cbor",
        refusal_body(code),
    )
    .await;
    get_widgets(&client(addr).with_contracts(TABLE))
        .await
        .expect_err("426")
}

#[tokio::test]
async fn an_unsigned_426_with_the_rest_code_is_contract_unsupported() {
    match answer_426(CONTRACT_UNSUPPORTED_REST_CODE).await {
        ClientError::Envelope(EnvelopeError::ContractUnsupported { op }) => {
            assert_eq!(
                op, "GET /widgets",
                "a REST op is named by method and template"
            );
        }
        other => panic!("expected ContractUnsupported, got {other:?}"),
    }
    assert_eq!(
        EnvelopeError::ContractUnsupported { op: String::new() }.code(),
        "envelope_contract_unsupported"
    );
}

#[tokio::test]
async fn an_unsigned_426_with_the_rpc_code_is_contract_unsupported() {
    let error = answer_426(CONTRACT_UNSUPPORTED_CODE).await;
    assert!(
        matches!(
            error,
            ClientError::Envelope(EnvelopeError::ContractUnsupported { .. })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_unsigned_426_with_any_other_code_stays_unsigned() {
    // A proxy's own 426 (it wants TLS or h2) is not a contract refusal.
    let error = answer_426("upgrade_required").await;
    assert!(
        matches!(
            error,
            ClientError::Envelope(EnvelopeError::Unsigned { status: 426 })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_unsigned_426_whose_body_is_not_a_refusal_stays_unsigned() {
    let (addr, _) = stub(StatusCode::UPGRADE_REQUIRED, "\u{0}not cbor, not json").await;
    let error = get_widgets(&client(addr).with_contracts(TABLE))
        .await
        .expect_err("426");
    assert!(
        matches!(
            error,
            ClientError::Envelope(EnvelopeError::Unsigned { status: 426 })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn any_other_unsigned_answer_is_still_unsigned() {
    let (addr, _) = stub(StatusCode::UNAUTHORIZED, "no").await;
    let client = client(addr).with_contracts(TABLE);
    let error = get_widgets(&client).await.expect_err("401");
    assert!(
        matches!(
            error,
            ClientError::Envelope(EnvelopeError::Unsigned { status: 401 })
        ),
        "{error:?}"
    );
}
