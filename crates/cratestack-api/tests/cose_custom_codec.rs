//! vpay's exact shape, proven upstream (cratestack#1168): a hand-written
//! service whose routes take a form and answer JSON (Stripe's shape), served
//! through `RestBindingResolver` over a static descriptor table, behind a
//! `Required` envelope layer that opted in to exactly those types. Reached
//! by a client with a hand-written form-in/JSON-out codec, and by a caller
//! doing its own HTTP with `seal_call` / `PendingResponse::open`.

mod cose_client_support;

use cose_client_support::{AUDIENCE, Kind, client_envelope, runtime_with, serve, server_envelope};
use cratestack::axum::Router;
use cratestack::axum::extract::Path;
use cratestack::axum::http::{HeaderMap, StatusCode, header};
use cratestack::axum::response::{IntoResponse, Response};
use cratestack::axum::routing::{get, post};
use cratestack::envelope_layer::{EnvelopeLayer, EnvelopeMode};
use cratestack::{
    AcceptedContracts, CratestackCodec, CratestackError, RouteTransportCapabilities,
    RouteTransportDescriptor,
};
use cratestack_client_rust::{ClientError, EnvelopeError, HttpClientCodec, RouteRef, SealCall};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const FORM: &str = "application/x-www-form-urlencoded";
const JSON: &str = "application/json";
const CONTRACT: [u8; 32] = [0x77; 32];
const KIND: Kind = Kind::Ed25519;

/// The service's own descriptors: forms in, JSON out, on every route.
const CAPS: RouteTransportCapabilities = RouteTransportCapabilities {
    request_types: &[FORM],
    response_types: &[JSON],
    default_response_type: JSON,
    supports_sequence_response: false,
};

const fn route(method: &'static str, path: &'static str) -> RouteTransportDescriptor {
    RouteTransportDescriptor {
        name: path,
        method,
        path,
        capabilities: CAPS,
        idempotent_by_default: false,
        rate_limited_by_default: true,
    }
}

static ROUTES: [RouteTransportDescriptor; 2] = [
    route("POST", "/v1/charges"),
    route("GET", "/v1/charges/{id}"),
];

static CONTRACTS: AcceptedContracts = &[
    ("POST /v1/charges", &[CONTRACT]),
    ("GET /v1/charges/{id}", &[CONTRACT]),
];

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Charge {
    amount: i64,
    currency: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Created {
    id: String,
    amount: i64,
    seen_content_type: String,
}

fn json(status: StatusCode, body: serde_json::Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body.to_string(),
    )
        .into_response()
}

async fn create(headers: HeaderMap, body: String) -> Response {
    let seen = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("<absent>");
    let charge: Charge = serde_urlencoded::from_str(&body).expect("a form");
    if charge.amount == 0 {
        // Stripe's own error envelope, which the layer must not re-encode.
        return json(
            StatusCode::PAYMENT_REQUIRED,
            serde_json::json!({ "error": { "type": "card_error", "code": "amount_zero" } }),
        );
    }
    json(
        StatusCode::OK,
        serde_json::json!({ "id": "ch_1", "amount": charge.amount, "seen_content_type": seen }),
    )
}

async fn fetch(Path(id): Path<String>) -> Response {
    match id.as_str() {
        // A framework-shaped refusal in a type nobody negotiated.
        "missing" => (StatusCode::NOT_FOUND, "no such charge").into_response(),
        id => json(
            StatusCode::OK,
            serde_json::json!({ "id": id, "amount": 1, "seen_content_type": "<absent>" }),
        ),
    }
}

async fn service() -> std::net::SocketAddr {
    let layer = EnvelopeLayer::builder(server_envelope(KIND), AUDIENCE, CONTRACTS)
        .policy(EnvelopeMode::Required)
        .rest("", &ROUTES)
        .payload_media_types([FORM], [JSON])
        .build()
        .expect("layer");
    serve(
        Router::new()
            .route("/v1/charges", post(create))
            .route("/v1/charges/{id}", get(fetch))
            .layer(layer),
    )
    .await
}

/// Form out, JSON back.
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

fn client(addr: std::net::SocketAddr) -> cratestack_client_rust::CratestackClient<FormJson> {
    runtime_with(addr, FormJson, client_envelope(KIND, AUDIENCE)).with_contract_sha(CONTRACT)
}

fn charge(amount: i64) -> Charge {
    Charge {
        amount,
        currency: "xaf".to_owned(),
    }
}

#[tokio::test]
async fn a_form_in_json_out_call_is_sealed_both_ways() {
    let client = client(service().await);
    let created: Created = client
        .at(RouteRef::new("/v1/charges", &[]))
        .post("/v1/charges", &charge(1500), &[])
        .await
        .expect("sealed both ways");
    assert_eq!(created.id, "ch_1");
    assert_eq!(created.amount, 1500);
    // The handler saw an ordinary form POST.
    assert_eq!(created.seen_content_type, FORM);
}

#[tokio::test]
async fn a_bodiless_get_with_a_path_parameter_is_sealed_too() {
    let client = client(service().await);
    let created: Created = client
        .at(RouteRef::new("/v1/charges/{id}", &["ch_7"]))
        .get("/v1/charges/ch_7", &[], &[])
        .await
        .expect("sealed both ways");
    assert_eq!(created.id, "ch_7");
}

/// Send `payload` (a form) to `POST /v1/charges` by hand, as a caller doing
/// its own HTTP does, and open the answer.
async fn post_charge(
    addr: std::net::SocketAddr,
    payload: &[u8],
) -> (u16, cratestack_client_rust::OpenedResponse) {
    let route = RouteRef::new("/v1/charges", &[]);
    let sealed = client_envelope(KIND, AUDIENCE)
        .seal_call(
            SealCall::new("POST", route, CONTRACT)
                .payload(payload, FORM)
                .accept(JSON),
        )
        .await
        .expect("sealed");
    let mut request = reqwest::Client::new()
        .post(format!("http://{addr}/v1/charges"))
        .body(sealed.body.clone());
    for (name, value) in &sealed.headers {
        request = request.header(name, value);
    }
    let response = request.send().await.expect("sent");
    let (status, headers) = (response.status().as_u16(), response.headers().clone());
    let opened = sealed
        .pending
        .open(status, &headers, response.bytes().await.expect("body"))
        .await
        .expect("a Required layer answers a seal_call, and it opens");
    (status, opened)
}

#[tokio::test]
async fn the_services_own_json_error_envelope_is_sealed_as_it_is() {
    let addr = service().await;
    // Through the typed client it is a 402.
    let error = client(addr)
        .at(RouteRef::new("/v1/charges", &[]))
        .post::<_, serde_json::Value>("/v1/charges", &charge(0), &[])
        .await
        .expect_err("a 402");
    assert!(
        matches!(
            error,
            ClientError::Remote {
                status: StatusCode::PAYMENT_REQUIRED,
                ..
            }
        ),
        "{error:?}"
    );
    // And byte for byte the service's, not the framework's error shape.
    let (status, opened) = post_charge(addr, b"amount=0&currency=xaf").await;
    assert_eq!(status, 402);
    assert_eq!(opened.payload_type, JSON);
    let body: serde_json::Value = serde_json::from_slice(&opened.body).expect("JSON");
    assert_eq!(body["error"]["code"], "amount_zero");
    assert!(body.get("message").is_none(), "not re-encoded: {body}");
}

#[tokio::test]
async fn a_text_error_is_re_encoded_in_json_and_sealed() {
    let client = client(service().await);
    let error = client
        .at(RouteRef::new("/v1/charges/{id}", &["missing"]))
        .get::<serde_json::Value>("/v1/charges/missing", &[], &[])
        .await
        .expect_err("a 404");
    let text = format!("{error:?}");
    assert!(text.contains("404") && text.contains("NOT_FOUND"), "{text}");
}

#[tokio::test]
async fn a_cbor_client_is_refused_unsigned_by_a_service_that_takes_forms_only() {
    let addr = service().await;
    let client = runtime_with(
        addr,
        cratestack_client_rust::CborCodec,
        client_envelope(KIND, AUDIENCE),
    )
    .with_contract_sha(CONTRACT);
    let error = client
        .at(RouteRef::new("/v1/charges", &[]))
        .post::<_, serde_json::Value>("/v1/charges", &charge(1), &[])
        .await
        .expect_err("415");
    assert!(
        matches!(
            error,
            ClientError::Envelope(EnvelopeError::Unsigned { status: 415 })
                | ClientError::Envelope(EnvelopeError::Unsigned { status: 406 })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_call_sealed_with_seal_call_and_sent_by_plain_reqwest_is_accepted_and_opens() {
    let (status, opened) = post_charge(service().await, b"amount=250&currency=xaf").await;
    assert_eq!(status, 200);
    assert_eq!(opened.payload_type, JSON);
    let created: Created = serde_json::from_slice(&opened.body).expect("JSON");
    assert_eq!(created.amount, 250);
}
