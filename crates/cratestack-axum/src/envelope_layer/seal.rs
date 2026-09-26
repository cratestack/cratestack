//! The response side: every response of a verified (or nonce-bound)
//! request is sealed against the binding its request was opened with.

use axum::body::Body;
use axum::response::Response;
use cratestack_core::{
    CratestackError, MAX_RESPONSE_REBUFFER_BYTES, RequestDigest, ResponseBinding,
};
use http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use http::{HeaderMap, HeaderValue, StatusCode};

pub(super) use super::inputs::BindingInputs;
use super::opened::SealContext;
use super::{media, refusal};
use crate::transport::StreamedResponseMarker;

/// Seals the response to one request.
pub(super) struct Sealer {
    pub(super) inputs: BindingInputs,
    pub(super) request: RequestDigest,
    pub(super) context: SealContext,
    /// A signed request, or `Required`: nothing leaves unsealed (S3).
    pub(super) strict: bool,
    /// The headers the router saw (`Accept: application/cbor` when
    /// strict), for encoding an error the layer seals itself.
    pub(super) headers: HeaderMap,
    pub(super) path: String,
}

impl Sealer {
    /// Seal `response`. A stream cannot be sealed (P1): when strict it is
    /// replaced by a sealed `406`, otherwise it passes plain. A non-CBOR
    /// error (axum's own `413`, a `text/plain` fallback) is re-encoded as the
    /// transport's CBOR error with the same status first. A non-CBOR
    /// success is a sealed `500` when strict and passes plain otherwise.
    ///
    /// The body is re-buffered up to `MAX_RESPONSE_REBUFFER_BYTES`; a
    /// longer one is replaced by a sealed `500`.
    pub(super) async fn finish(self, response: Response) -> Response {
        if response
            .extensions()
            .get::<StreamedResponseMarker>()
            .is_some()
        {
            if !self.strict {
                return response;
            }
            return self
                .seal_error(StatusCode::NOT_ACCEPTABLE, stream_refused())
                .await;
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
        if !self.strict {
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
        let config = &self.inputs.config;
        let sealed = match config
            .envelope
            .seal_response(payload, &bind, &self.context)
            .await
        {
            Ok(sealed) => sealed,
            Err(error) => return self.unsigned_internal(&error),
        };
        let (body, media_type) = sealed.into_parts();
        // The envelope names the type; the layer refuses one that would
        // label a sealed body as anything a client reads as plain.
        let content_type = match HeaderValue::from_str(&media_type) {
            Ok(value) if media::is_envelope_media_type(&media_type, &*config.envelope) => value,
            _ => {
                return self.unsigned_internal(&CratestackError::Internal(format!(
                    "the envelope sealed a response as {media_type:?}, not an envelope type"
                )));
            }
        };
        parts.headers.remove(CONTENT_LENGTH);
        parts.headers.insert(CONTENT_TYPE, content_type);
        Response::from_parts(parts, Body::from(body))
    }

    /// The one response that cannot be signed: signing itself failed.
    fn unsigned_internal(&self, error: &CratestackError) -> Response {
        refusal::internal(&self.headers, &self.path, "seal", error)
    }
}

pub(super) fn stream_refused() -> CratestackError {
    CratestackError::NotAcceptable(
        "a streamed response cannot be signed yet (ADR 0006 P1)".to_owned(),
    )
}
