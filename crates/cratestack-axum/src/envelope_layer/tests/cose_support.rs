//! Keys, envelopes, and the client side of a signed exchange (feature
//! `cose`).

use std::borrow::Cow;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use bytes::Bytes;
use cratestack_core::{
    Binding, BoundHeaders, CratestackError, InMemoryNonceStore, PathParams, RequestDigest,
    ResponseBinding,
};
use cratestack_cose::{CoseEnvelope, CoseMode, Ed25519Signer, StaticVerifierResolver};
use http::{Method, StatusCode, header};

use super::support::{AUDIENCE, SCHEMA, SIGN1};

/// `keys.json`'s `ed25519` (the client) and `ed25519_other` (the server).
fn seed(start: u8) -> [u8; 32] {
    std::array::from_fn(|index| start + index as u8)
}

pub fn client_signer() -> Ed25519Signer {
    Ed25519Signer::from_seed(&seed(0x00))
}

pub fn server_signer() -> Ed25519Signer {
    Ed25519Signer::from_seed(&seed(0x80))
}

/// `cose:` + `keys.json`'s `ed25519` thumbprint.
pub const CLIENT_PRINCIPAL: &str =
    "cose:be5de2f4bcdc383add3fc9827d345f1a37c6a06026b38696fb3229c003b35f49";

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

pub fn client_envelope() -> CoseEnvelope {
    let resolver = StaticVerifierResolver::new().with_key(server_signer().verify_key());
    CoseEnvelope::client(
        CoseMode::Sign1,
        Arc::new(client_signer()),
        Arc::new(resolver),
    )
    .build()
    .expect("client envelope")
}

/// One call as the client binds it.
pub struct Call {
    pub method: Method,
    pub route: &'static str,
    pub params: Vec<&'static str>,
    pub query: Option<&'static str>,
    /// `bound_headers`: `Idempotency-Key`, `If-Match`, as the client binds
    /// them.
    pub bound: (Option<&'static str>, Option<&'static str>),
}

impl Call {
    pub fn new(method: Method, route: &'static str, params: &[&'static str]) -> Self {
        Self {
            method,
            route,
            params: params.to_vec(),
            query: None,
            bound: (None, None),
        }
    }

    /// The same call, binding `Idempotency-Key` and `If-Match`.
    pub fn bound(
        self,
        idempotency_key: Option<&'static str>,
        if_match: Option<&'static str>,
    ) -> Self {
        Self {
            bound: (idempotency_key, if_match),
            ..self
        }
    }

    pub fn binding(&self, response: Option<ResponseBinding>) -> Binding<'_> {
        Binding {
            audience: Cow::Borrowed(AUDIENCE),
            method: Cow::Borrowed(self.method.as_str()),
            route: Cow::Borrowed(self.route),
            path_params: PathParams::Borrowed(&self.params),
            query: self.query.map(Cow::Borrowed),
            schema_sha: SCHEMA,
            payload_media_type: Cow::Borrowed("application/cbor"),
            bound_headers: BoundHeaders {
                idempotency_key: self.bound.0.map(Cow::Borrowed),
                if_match: self.bound.1.map(Cow::Borrowed),
            },
            response,
        }
    }

    pub async fn seal(&self, payload: &[u8]) -> Bytes {
        client_envelope()
            .seal_request(payload, &self.binding(None))
            .await
            .expect("seal the request")
    }

    /// Open a sealed response to the request whose digest is `request`.
    pub async fn open(
        &self,
        request: RequestDigest,
        status: StatusCode,
        body: Bytes,
    ) -> Result<Bytes, CratestackError> {
        let response = ResponseBinding {
            request,
            status: status.as_u16(),
        };
        client_envelope()
            .open_response(body, &self.binding(Some(response)))
            .await
            .map(|opened| opened.payload)
    }
}

pub fn cose_request(method: Method, uri: &str, body: Bytes) -> Request {
    http::Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, SIGN1)
        .header(header::ACCEPT, SIGN1)
        .body(Body::from(body))
        .expect("request")
}
