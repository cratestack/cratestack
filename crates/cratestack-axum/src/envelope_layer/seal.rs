//! The binding, built once per request, and the response side: every
//! response of a verified (or nonce-bound) request is sealed against it.

use std::borrow::Cow;
use std::sync::Arc;

use axum::body::Body;
use axum::response::Response;
use cratestack_core::{
    Binding, CratestackError, MAX_RESPONSE_REBUFFER_BYTES, PathParams, RequestDigest,
    ResponseBinding, canonical_query,
};
use http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use http::{HeaderMap, Method, StatusCode, Uri};

use super::layer::Config;
use super::mode::EnvelopeMode;
use super::resolver::ResolvedRoute;
use super::{PAYLOAD_MEDIA_TYPE, media, refusal};
use crate::transport::StreamedResponseMarker;

/// The request's half of the binding. Built once from the request and the
/// single [`ResolvedRoute`], used to open the request and, unchanged, to
/// seal its response: no plug-in is asked twice.
pub(super) struct BindingInputs {
    pub(super) config: Arc<Config>,
    pub(super) method: Method,
    pub(super) route: ResolvedRoute,
    query: Option<String>,
}

impl BindingInputs {
    pub(super) fn new(
        config: Arc<Config>,
        method: Method,
        route: ResolvedRoute,
        uri: &Uri,
    ) -> Self {
        // `canonical_query` maps an absent query to `""`; the AAD encodes a
        // query-less request as `null`, and `cratestack-cose` reads an empty
        // one the same way.
        let query = Some(canonical_query(uri.query())).filter(|query| !query.is_empty());
        Self {
            config,
            method,
            route,
            query,
        }
    }

    pub(super) fn params(&self) -> Vec<&str> {
        self.route
            .path_params()
            .iter()
            .map(String::as_str)
            .collect()
    }

    pub(super) fn binding<'a>(
        &'a self,
        params: &'a [&'a str],
        response: Option<ResponseBinding>,
    ) -> Binding<'a> {
        Binding {
            audience: Cow::Borrowed(&self.config.audience),
            method: Cow::Borrowed(self.method.as_str()),
            route: Cow::Borrowed(self.route.route()),
            path_params: PathParams::Borrowed(params),
            query: self.query.as_deref().map(Cow::Borrowed),
            schema_sha: self.config.schema_sha,
            payload_media_type: Cow::Borrowed(PAYLOAD_MEDIA_TYPE),
            response,
        }
    }
}

/// Seals the response to one request.
pub(super) struct Sealer {
    pub(super) inputs: BindingInputs,
    pub(super) request: RequestDigest,
    pub(super) mode: EnvelopeMode,
    /// The headers the router saw (`Accept: application/cbor` under
    /// `Required`), for encoding an error the layer seals itself.
    pub(super) headers: HeaderMap,
    pub(super) path: String,
}

impl Sealer {
    /// Seal `response`. A stream cannot be sealed (P1): under `Required` it
    /// is replaced by a sealed `406`, otherwise it passes plain. A non-CBOR
    /// error (axum's own `413`, a `text/plain` fallback) is re-encoded as the
    /// transport's CBOR error with the same status first. A non-CBOR
    /// success is a sealed `500` under `Required` and passes plain otherwise.
    pub(super) async fn finish(self, response: Response) -> Response {
        if response
            .extensions()
            .get::<StreamedResponseMarker>()
            .is_some()
        {
            if self.mode != EnvelopeMode::Required {
                return response;
            }
            let error = CratestackError::NotAcceptable(
                "a streamed response cannot be signed yet (ADR 0006 P1)".to_owned(),
            );
            return self.seal_error(error.status_code(), error).await;
        }
        let (parts, body) = response.into_parts();
        let payload = match axum::body::to_bytes(body, MAX_RESPONSE_REBUFFER_BYTES).await {
            Ok(payload) => payload,
            Err(error) => {
                let error = CratestackError::Internal(format!("buffer the response: {error}"));
                return self
                    .seal_error(StatusCode::INTERNAL_SERVER_ERROR, error)
                    .await;
            }
        };
        if media::is_cbor_response(&parts.headers) {
            return self.seal(parts, payload.as_ref()).await;
        }
        if !parts.status.is_success() {
            let error = crate::rpc::util::synthesize_error_for_status(parts.status);
            return self.seal_error(parts.status, error).await;
        }
        if self.mode != EnvelopeMode::Required {
            return Response::from_parts(parts, Body::from(payload));
        }
        let error = CratestackError::Internal("a success response that is not CBOR".to_owned());
        self.seal_error(StatusCode::INTERNAL_SERVER_ERROR, error)
            .await
    }

    /// Seal an error the layer raises itself, in the transport's error
    /// shape, CBOR-encoded, at `status`.
    pub(super) async fn seal_error(&self, status: StatusCode, error: CratestackError) -> Response {
        if status.is_server_error() {
            tracing::error!(
                target: "cratestack",
                cratestack_operation = "envelope",
                cratestack_error = error.code(),
                cratestack_detail = error.detail().unwrap_or(""),
                "envelope layer replaced a response with a sealed error",
            );
        }
        let mut headers = self.headers.clone();
        headers.insert(http::header::ACCEPT, media::cbor_header_value());
        let encoded = crate::middleware_error::middleware_error_response_with_status(
            &headers, &self.path, status, error,
        );
        let (parts, body) = encoded.into_parts();
        match axum::body::to_bytes(body, MAX_RESPONSE_REBUFFER_BYTES).await {
            Ok(payload) if media::is_cbor_response(&parts.headers) => {
                self.seal(parts, payload.as_ref()).await
            }
            _ => self.unsigned_internal(&CratestackError::Internal(
                "encode a sealed error".to_owned(),
            )),
        }
    }

    async fn seal(&self, mut parts: http::response::Parts, payload: &[u8]) -> Response {
        let params = self.inputs.params();
        let response = ResponseBinding {
            request: self.request,
            status: parts.status.as_u16(),
        };
        let bind = self.inputs.binding(&params, Some(response));
        match self
            .inputs
            .config
            .envelope
            .seal_response(payload, &bind)
            .await
        {
            Ok(sealed) => {
                parts.headers.remove(CONTENT_LENGTH);
                parts
                    .headers
                    .insert(CONTENT_TYPE, self.inputs.config.media_type.clone());
                Response::from_parts(parts, Body::from(sealed))
            }
            Err(error) => self.unsigned_internal(&error),
        }
    }

    /// The one response that cannot be signed: signing itself failed.
    fn unsigned_internal(&self, error: &CratestackError) -> Response {
        refusal::internal(&self.headers, &self.path, "seal", error)
    }
}
