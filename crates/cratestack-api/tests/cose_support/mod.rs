//! Shared by `cose_envelope_rest.rs` and `cose_envelope_rpc.rs`: the client
//! side of a signed exchange (with the published test keys of
//! `cratestack-cose/tests/vectors/keys.json`; never use them elsewhere),
//! and the two stores the §12 placement tests put behind the envelope.

#![allow(dead_code)]

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
    Binding, CratestackCodec, CratestackError, InMemoryNonceStore, PathParams, ResponseBinding,
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

pub fn envelope_layer(schema_sha: [u8; 32]) -> EnvelopeLayerBuilder {
    let resolver = StaticVerifierResolver::new().with_key(client_signer().verify_key());
    let server = CoseEnvelope::server(
        CoseMode::Sign1,
        Arc::new(server_signer()),
        Arc::new(resolver),
        Arc::new(InMemoryNonceStore::new()),
    )
    .build()
    .expect("server envelope");
    EnvelopeLayer::builder(server, AUDIENCE, schema_sha).policy(EnvelopeMode::Required)
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

impl Call {
    fn binding(&self, response: Option<ResponseBinding>) -> Binding<'_> {
        Binding {
            audience: Cow::Borrowed(AUDIENCE),
            method: Cow::Borrowed("POST"),
            route: Cow::Borrowed(self.route),
            path_params: PathParams::EMPTY,
            query: None,
            schema_sha: self.schema_sha,
            payload_media_type: Cow::Borrowed("application/cbor"),
            response,
        }
    }

    /// A signed `POST` of `payload` to `uri`, with extra headers.
    pub async fn request(
        &self,
        uri: &str,
        payload: &[u8],
        extra: &[(&str, &str)],
    ) -> (Bytes, Request<Body>) {
        let sealed = client()
            .seal_request(payload, &self.binding(None))
            .await
            .expect("seal");
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(header::CONTENT_TYPE, SIGN1)
            .header(header::ACCEPT, SIGN1);
        for (name, value) in extra {
            builder = builder.header(*name, *value);
        }
        (
            sealed.clone(),
            builder.body(Body::from(sealed)).expect("request"),
        )
    }

    /// Verify a sealed response to the request `sealed`, returning its payload.
    pub async fn open(&self, sealed: &[u8], answer: &Answer) -> Result<Bytes, CratestackError> {
        let response = ResponseBinding {
            request: request_digest(sealed),
            status: answer.status.as_u16(),
        };
        client()
            .open_response(answer.body.clone(), &self.binding(Some(response)))
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
