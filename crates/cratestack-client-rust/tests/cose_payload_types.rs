//! A sealed call may carry a payload that is not CBOR (cratestack#1168): the
//! client names the types in `Cratestack-Payload-Type` / `-Accept`, binds
//! them, and never decodes a type it did not ask for.
//!
//! The server is a stub that seals whatever the test tells it to, with a
//! real server envelope, so the bytes asserted on are the signed ones.

#![cfg(feature = "cose")]

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, HeaderValue};
use axum::routing::post;
use cratestack_client_rust::cose::{
    CoseAlg, CoseEnvelope, CoseMode, HmacSigner, StaticVerifierResolver, request_digest,
};
use cratestack_client_rust::{
    CborCodec, ClientConfig, ClientEnvelope, ClientError, CratestackClient, EnvelopeError,
    HttpClientCodec, JsonCodec, RouteRef, ensure_crypto_provider,
};
use cratestack_core::{
    Binding, BoundHeaders, CratestackCodec, CratestackError, InMemoryNonceStore,
    PAYLOAD_ACCEPT_HEADER, PAYLOAD_TYPE_HEADER, PathParams, ResponseBinding,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

const FORM: &str = "application/x-www-form-urlencoded";
const JSON: &str = "application/json";
const CBOR: &str = "application/cbor";
const CONTRACT: [u8; 32] = [0x44; 32];

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Charge {
    amount: i64,
    currency: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Reply {
    ok: bool,
}

/// vpay's shape: a form on the way out, JSON on the way back.
#[derive(Clone)]
struct FormJson;

impl CratestackCodec for FormJson {
    const CONTENT_TYPE: &'static str = FORM;

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CratestackError> {
        serde_urlencoded::to_string(value)
            .map(String::into_bytes)
            .map_err(|error| CratestackError::Codec(error.to_string()))
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, CratestackError> {
        serde_json::from_slice(bytes).map_err(|error| CratestackError::Codec(error.to_string()))
    }
}

impl HttpClientCodec for FormJson {
    fn accept_header_value(&self) -> &'static str {
        JSON
    }

    fn sequence_accept_header_value(&self) -> &'static str {
        JSON
    }

    fn payload_accept(&self) -> &'static str {
        JSON
    }

    fn decode_response<T: DeserializeOwned>(
        &self,
        content_type: &str,
        body: &[u8],
    ) -> Result<T, CratestackError> {
        match content_type.split(';').next().map(str::trim) {
            Some(JSON) => self.decode(body),
            _ => Err(CratestackError::Codec(format!("unexpected {content_type}"))),
        }
    }

    fn decode_sequence_response<T: DeserializeOwned>(
        &self,
        content_type: &str,
        body: &[u8],
    ) -> Result<Vec<T>, CratestackError> {
        self.decode_response(content_type, body)
    }
}

fn signer() -> HmacSigner {
    HmacSigner::new(CoseAlg::Hmac256_64, vec![9; 32]).unwrap()
}

fn resolver() -> Arc<StaticVerifierResolver> {
    Arc::new(StaticVerifierResolver::new().with_key(signer().verify_key()))
}

fn client_envelope() -> ClientEnvelope {
    let cose = CoseEnvelope::client(CoseMode::Mac0, Arc::new(signer()), resolver())
        .build()
        .unwrap();
    ClientEnvelope::new(cose, "payments").unwrap()
}

fn server_envelope() -> CoseEnvelope {
    CoseEnvelope::server(
        CoseMode::Mac0,
        Arc::new(signer()),
        resolver(),
        Arc::new(InMemoryNonceStore::new()),
    )
    .build()
    .unwrap()
}

/// What the stub seals back.
#[derive(Clone)]
struct Answer {
    /// The type the response binding names.
    bound: &'static str,
    /// What `Cratestack-Payload-Type` says on the way back (`None`: absent).
    says: Option<&'static str>,
    payload: Vec<u8>,
}

impl Answer {
    fn honest(content_type: &'static str, payload: &[u8]) -> Self {
        Self {
            bound: content_type,
            says: Some(content_type),
            payload: payload.to_vec(),
        }
    }
}

type Seen = Arc<Mutex<Vec<(HeaderMap, Bytes)>>>;

async fn stub(answer: Answer) -> (std::net::SocketAddr, Seen) {
    ensure_crypto_provider();
    let seen: Seen = Arc::default();
    let recorded = seen.clone();
    let handler = move |headers: HeaderMap, body: Bytes| {
        let (recorded, answer) = (recorded.clone(), answer.clone());
        async move {
            recorded.lock().unwrap().push((headers, body.clone()));
            let bind = Binding {
                response: Some(ResponseBinding {
                    request: request_digest(&body),
                    status: 200,
                }),
                ..binding(answer.bound)
            };
            let sealed = server_envelope()
                .seal_response(&answer.payload, &bind)
                .await
                .unwrap();
            let mut headers = HeaderMap::new();
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static(CoseMode::Mac0.media_type()),
            );
            if let Some(says) = answer.says {
                headers.insert(PAYLOAD_TYPE_HEADER, HeaderValue::from_static(says));
            }
            (headers, sealed)
        }
    };
    let router = Router::new()
        .route("/pay", post(handler.clone()))
        .route("/rpc/batch", post(handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (addr, seen)
}

fn binding(payload_type: &'static str) -> Binding<'static> {
    Binding {
        audience: Cow::Borrowed("payments"),
        method: Cow::Borrowed("POST"),
        route: Cow::Borrowed("/pay"),
        path_params: PathParams::EMPTY,
        query: None,
        contract_sha: CONTRACT,
        payload_media_type: Cow::Borrowed(payload_type),
        bound_headers: BoundHeaders::NONE,
        response: None,
    }
}

fn client<C: HttpClientCodec>(addr: std::net::SocketAddr, codec: C) -> CratestackClient<C> {
    CratestackClient::new(
        ClientConfig::new(Url::parse(&format!("http://{addr}")).unwrap()),
        codec,
    )
    .with_envelope(client_envelope())
    .unwrap()
    .with_contract_sha(CONTRACT)
}

fn charge() -> Charge {
    Charge {
        amount: 1500,
        currency: "xaf".to_owned(),
    }
}

async fn pay<C: HttpClientCodec>(client: &CratestackClient<C>) -> Result<Reply, ClientError> {
    client
        .at(RouteRef::new("/pay", &[]))
        .post::<_, Reply>("/pay", &charge(), &[])
        .await
}

fn envelope_error(error: ClientError) -> EnvelopeError {
    match error {
        ClientError::Envelope(error) => error,
        other => panic!("expected an envelope error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_form_in_json_out_client_names_both_types_and_binds_them() {
    let (addr, seen) = stub(Answer::honest(JSON, b"{\"ok\":true}")).await;
    let reply = pay(&client(addr, FormJson))
        .await
        .expect("sealed both ways");
    assert_eq!(reply, Reply { ok: true });

    let (headers, sealed) = seen.lock().unwrap().remove(0);
    assert_eq!(headers[PAYLOAD_TYPE_HEADER].to_str().unwrap(), FORM);
    assert_eq!(headers[PAYLOAD_ACCEPT_HEADER].to_str().unwrap(), JSON);
    // The request is sealed under the form type and no other.
    server_envelope()
        .open_request(sealed.clone(), &binding(CBOR))
        .await
        .expect_err("not CBOR");
    let opened = server_envelope()
        .open_request(sealed, &binding(FORM))
        .await
        .expect("sealed under the form type");
    assert_eq!(opened.payload.as_ref(), b"amount=1500&currency=xaf");
}

#[tokio::test]
async fn a_cbor_client_sends_neither_selector_header_and_seals_under_cbor() {
    let (addr, seen) = stub(Answer::honest(
        CBOR,
        &CborCodec.encode(&Reply2 { ok: true }).unwrap(),
    ))
    .await;
    let reply: Reply = client(addr, CborCodec)
        .at(RouteRef::new("/pay", &[]))
        .post("/pay", &charge(), &[])
        .await
        .expect("round trip");
    assert_eq!(reply, Reply { ok: true });
    let (headers, sealed) = seen.lock().unwrap().remove(0);
    assert!(!headers.contains_key(PAYLOAD_TYPE_HEADER), "0.15.3 wire");
    assert!(!headers.contains_key(PAYLOAD_ACCEPT_HEADER), "0.15.3 wire");
    server_envelope()
        .open_request(sealed, &binding(CBOR))
        .await
        .expect("CBOR, as ever");
}

#[derive(Serialize)]
struct Reply2 {
    ok: bool,
}

#[tokio::test]
async fn a_json_codec_client_can_take_an_envelope() {
    let (addr, seen) = stub(Answer::honest(JSON, b"{\"ok\":true}")).await;
    assert_eq!(
        pay(&client(addr, JsonCodec)).await.expect("JSON both ways"),
        Reply { ok: true }
    );
    let (headers, sealed) = seen.lock().unwrap().remove(0);
    assert_eq!(headers[PAYLOAD_TYPE_HEADER].to_str().unwrap(), JSON);
    assert_eq!(headers[PAYLOAD_ACCEPT_HEADER].to_str().unwrap(), JSON);
    server_envelope()
        .open_request(sealed, &binding(JSON))
        .await
        .expect("sealed under JSON");
}

#[tokio::test]
async fn a_response_type_that_was_not_asked_for_is_refused_unread() {
    // The server seals CBOR, honestly labelled, to a client that reads JSON.
    let cbor = CborCodec.encode(&Reply2 { ok: true }).unwrap();
    for says in [Some(CBOR), None] {
        let (addr, _) = stub(Answer {
            bound: CBOR,
            says,
            payload: cbor.clone(),
        })
        .await;
        let error = envelope_error(pay(&client(addr, FormJson)).await.expect_err("refused"));
        assert!(
            matches!(&error, EnvelopeError::UnexpectedPayloadType { got } if got == CBOR),
            "{says:?}: {error:?}"
        );
        assert_eq!(error.code(), "envelope_unexpected_payload_type");
    }
}

#[tokio::test]
async fn an_echo_that_lies_about_the_sealed_type_is_unverified() {
    // Sealed as CBOR, but says JSON (which the client did ask for).
    let (addr, _) = stub(Answer {
        bound: CBOR,
        says: Some(JSON),
        payload: b"{\"ok\":true}".to_vec(),
    })
    .await;
    let error = envelope_error(pay(&client(addr, FormJson)).await.expect_err("tampered"));
    assert!(matches!(error, EnvelopeError::Unverified), "{error:?}");
}

#[tokio::test]
async fn an_echo_naming_a_malformed_or_repeated_type_is_refused() {
    // Not in the grammar, so it cannot be a type the client asked for.
    let (addr, _) = stub(Answer {
        bound: JSON,
        says: Some("Application/JSON"),
        payload: b"{\"ok\":true}".to_vec(),
    })
    .await;
    let error = envelope_error(pay(&client(addr, FormJson)).await.expect_err("malformed"));
    assert!(
        matches!(error, EnvelopeError::UnexpectedPayloadType { .. }),
        "{error:?}"
    );
}

#[derive(Clone)]
struct Streamy;

impl CratestackCodec for Streamy {
    const CONTENT_TYPE: &'static str = "text/event-stream";

    fn encode<T: Serialize + ?Sized>(&self, _: &T) -> Result<Vec<u8>, CratestackError> {
        Ok(Vec::new())
    }

    fn decode<T: DeserializeOwned>(&self, _: &[u8]) -> Result<T, CratestackError> {
        Err(CratestackError::Codec("no".to_owned()))
    }
}

impl HttpClientCodec for Streamy {
    fn accept_header_value(&self) -> &'static str {
        Self::CONTENT_TYPE
    }

    fn sequence_accept_header_value(&self) -> &'static str {
        Self::CONTENT_TYPE
    }

    fn decode_response<T: DeserializeOwned>(
        &self,
        _: &str,
        _: &[u8],
    ) -> Result<T, CratestackError> {
        Err(CratestackError::Codec("no".to_owned()))
    }

    fn decode_sequence_response<T: DeserializeOwned>(
        &self,
        _: &str,
        _: &[u8],
    ) -> Result<Vec<T>, CratestackError> {
        Err(CratestackError::Codec("no".to_owned()))
    }
}

#[tokio::test]
async fn a_codec_whose_type_can_never_be_sealed_is_refused_up_front() {
    let client = CratestackClient::new(
        ClientConfig::new(Url::parse("http://127.0.0.1:1").unwrap()),
        Streamy,
    );
    let error = client
        .with_envelope(client_envelope())
        .err()
        .expect("refused");
    assert!(matches!(error, ClientError::BadInput(_)), "{error:?}");
}

#[tokio::test]
async fn a_batch_over_a_non_cbor_codec_is_bad_input_and_nothing_is_sent() {
    let (addr, seen) = stub(Answer::honest(JSON, b"[]")).await;
    for error in [
        client(addr, JsonCodec)
            .at(RouteRef::rpc("batch"))
            .post::<_, serde_json::Value>("/rpc/batch", &[0u8; 0], &[])
            .await
            .expect_err("batch is CBOR"),
        client(addr, FormJson)
            .at(RouteRef::rpc("batch"))
            .post::<_, serde_json::Value>("/rpc/batch", &charge(), &[])
            .await
            .expect_err("batch is CBOR"),
    ] {
        assert!(matches!(error, ClientError::BadInput(_)), "{error:?}");
    }
    assert!(seen.lock().unwrap().is_empty(), "never sent");
}
