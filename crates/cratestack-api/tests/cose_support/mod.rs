//! Shared by `cose_envelope_rest.rs` and `cose_envelope_rpc.rs`: the client
//! side of a signed exchange (with the published test keys of
//! `cratestack-cose/tests/vectors/keys.json`; never use them elsewhere),
//! and the two stores the §12 placement tests put behind the envelope.

// Each test binary uses a different subset of these helpers.
#![allow(dead_code, unused_imports)]

mod stores;

use std::borrow::Cow;
use std::sync::Arc;

use cratestack::axum::Router;
use cratestack::axum::body::{Body, Bytes, to_bytes};
use cratestack::axum::http::{HeaderMap, Method, Request, StatusCode, header};
use cratestack::cose::{
    CoseEnvelope, CoseMode, Ed25519Signer, StaticVerifierResolver, request_digest,
};
use cratestack::envelope_layer::{EnvelopeLayer, EnvelopeLayerBuilder, EnvelopeMode};
use cratestack::{
    Binding, BoundHeaders, CratestackCodec, CratestackError, InMemoryNonceStore, PathParams,
    ResponseBinding,
};
use cratestack_codec_cbor::CborCodec;
use tower::ServiceExt;

pub use stores::{MemoryIdempotency, SpyRateLimit};

pub const AUDIENCE: &str = "payments";
pub const SIGN1: &str = "application/cose; cose-type=\"cose-sign1\"";
/// `cose:` + `keys.json`'s `ed25519` thumbprint: the client's principal.
pub const CLIENT_PRINCIPAL: &str =
    "cose:be5de2f4bcdc383add3fc9827d345f1a37c6a06026b38696fb3229c003b35f49";

fn seed(start: u8) -> [u8; 32] {
    std::array::from_fn(|index| start + index as u8)
}

fn client_signer() -> Ed25519Signer {
    Ed25519Signer::from_seed(&seed(0x00))
}

fn server_signer() -> Ed25519Signer {
    Ed25519Signer::from_seed(&seed(0x80))
}

/// The server's envelope, trusting the client's key.
pub fn server_envelope() -> CoseEnvelope {
    let resolver = StaticVerifierResolver::new().with_key(client_signer().verify_key());
    CoseEnvelope::server(
        CoseMode::Sign1,
        Arc::new(server_signer()),
        Arc::new(resolver),
        Arc::new(InMemoryNonceStore::new()),
    )
    .build()
    .expect("server envelope")
}

pub fn envelope_layer(schema_sha: [u8; 32]) -> EnvelopeLayerBuilder {
    EnvelopeLayer::builder(server_envelope(), AUDIENCE, schema_sha).policy(EnvelopeMode::Required)
}

fn client() -> CoseEnvelope {
    let resolver = StaticVerifierResolver::new().with_key(server_signer().verify_key());
    CoseEnvelope::client(
        CoseMode::Sign1,
        Arc::new(client_signer()),
        Arc::new(resolver),
    )
    .build()
    .expect("client envelope")
}

/// One call, as the client binds it.
pub struct Call {
    pub route: &'static str,
    pub schema_sha: [u8; 32],
}

/// A sealed request as sent: its bytes, and the `Idempotency-Key` /
/// `If-Match` it carried, which its binding (and its response's) names.
pub struct Sent {
    pub bytes: Bytes,
    idempotency_key: Option<String>,
    if_match: Option<String>,
}

impl Sent {
    fn bound(&self) -> BoundHeaders<'_> {
        BoundHeaders {
            idempotency_key: self.idempotency_key.as_deref().map(Cow::Borrowed),
            if_match: self.if_match.as_deref().map(Cow::Borrowed),
        }
    }
}

impl Call {
    fn binding<'a>(
        &'a self,
        bound: BoundHeaders<'a>,
        response: Option<ResponseBinding>,
    ) -> Binding<'a> {
        Binding {
            audience: Cow::Borrowed(AUDIENCE),
            method: Cow::Borrowed("POST"),
            route: Cow::Borrowed(self.route),
            path_params: PathParams::EMPTY,
            query: None,
            schema_sha: self.schema_sha,
            payload_media_type: Cow::Borrowed("application/cbor"),
            bound_headers: bound,
            response,
        }
    }

    /// A signed `POST` of `payload` to `uri`, with extra headers; an
    /// `Idempotency-Key` or `If-Match` among them is bound, as a client
    /// binds what it sends (S1).
    pub async fn request(
        &self,
        uri: &str,
        payload: &[u8],
        extra: &[(&str, &str)],
    ) -> (Sent, Request<Body>) {
        let named = |name: &str| {
            extra
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| (*value).to_owned())
        };
        let mut sent = Sent {
            bytes: Bytes::new(),
            idempotency_key: named("idempotency-key"),
            if_match: named("if-match"),
        };
        sent.bytes = client()
            .seal_request(payload, &self.binding(sent.bound(), None))
            .await
            .expect("seal");
        let sealed = sent.bytes.clone();
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(header::CONTENT_TYPE, SIGN1)
            .header(header::ACCEPT, SIGN1);
        for (name, value) in extra {
            builder = builder.header(*name, *value);
        }
        (sent, builder.body(Body::from(sealed)).expect("request"))
    }

    /// Verify a sealed response to the request `sent`, returning its payload.
    pub async fn open(&self, sent: &Sent, answer: &Answer) -> Result<Bytes, CratestackError> {
        let response = ResponseBinding {
            request: request_digest(&sent.bytes),
            status: answer.status.as_u16(),
        };
        client()
            .open_response(
                answer.body.clone(),
                &self.binding(sent.bound(), Some(response)),
            )
            .await
            .map(|opened| opened.payload)
    }
}

pub struct Answer {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl Answer {
    pub fn is_sealed(&self) -> bool {
        self.headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            == Some(SIGN1)
    }
}

pub async fn send(router: &Router, req: Request<Body>) -> Answer {
    let response = router.clone().oneshot(req).await.expect("infallible");
    let (parts, body) = response.into_parts();
    Answer {
        status: parts.status,
        headers: parts.headers,
        body: to_bytes(body, usize::MAX).await.expect("body"),
    }
}

/// The CBOR error body's `code`.
pub fn error_code(body: &[u8]) -> String {
    let value: serde_json::Value = CborCodec.decode(body).expect("CBOR error body");
    value["code"].as_str().expect("code").to_owned()
}
