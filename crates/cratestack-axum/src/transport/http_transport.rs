use axum::http::StatusCode;
use axum::response::Response;
use cratestack_core::{CoolCodec, CoolError, CoolErrorResponse};
use futures_util::Stream;
use serde::{Deserialize, Serialize};

use crate::codec::encode_codec_response;

use super::CBOR_SEQUENCE_CONTENT_TYPE;
use super::internal::encode_cbor_sequence_response;
use super::media_type::media_type_matches;
use super::stream_sequence::encode_cbor_sequence_stream_response;

pub trait HttpTransport: Clone + Send + Sync + 'static {
    /// Whether this transport actually has an encoder for `content_type`
    /// (cratestack#489). [`RouteTransportCapabilities::response_types`] is
    /// a compile-time list of what the transport *shape* can carry (e.g.
    /// both CBOR and JSON, for every route), not what the concrete codec(s)
    /// wired into this particular router were built with — a router
    /// constructed with a single `JsonCodec` still emits a
    /// `response_types` list that names `application/cbor`. Response
    /// content-type negotiation (`select_response_content_type`) and the
    /// `Accept` preflight (`validate_transport_accept_header`) both filter
    /// their candidate list through this before matching against the
    /// client's `Accept` header, so the server can never select — or
    /// pre-approve, then later fail on — a `Content-Type` it has no
    /// encoder for.
    ///
    /// Defaulted rather than required: this trait is public API, and a
    /// required method would break any downstream `HttpTransport` impl
    /// that isn't one of the two in this crate. The default reports every
    /// content type as encodable, i.e. it preserves exactly the
    /// pre-cratestack#489 behavior (trust the static capability list) for
    /// any implementor that hasn't opted in yet. Both in-repo impls below
    /// override it with their real answer.
    fn can_encode(&self, _content_type: &str) -> bool {
        true
    }

    fn decode_request<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: for<'de> Deserialize<'de>;

    fn encode_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &T,
    ) -> Result<Response, CoolError>
    where
        T: Serialize + ?Sized;

    fn encode_sequence_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        values: &[T],
    ) -> Result<Response, CoolError>
    where
        T: Serialize;

    fn encode_sequence_error_response(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &CoolErrorResponse,
    ) -> Result<Response, CoolError>;

    /// Genuinely incremental counterpart to [`Self::encode_sequence_response`]
    /// for `@stream` procedures (cratestack#283): `values` is encoded and
    /// flushed item-by-item via `axum::body::Body::from_stream` instead
    /// of collected into a `Vec` first. Only meaningful for
    /// `application/cbor-seq` — implementations reject any other
    /// `content_type`, mirroring how the higher-level
    /// `encode_transport_stream_result_with_status_for` only calls this
    /// when cbor-seq was negotiated (anything else falls back to the
    /// buffered `encode_sequence_response` path there).
    fn encode_sequence_stream_response<T, S>(
        &self,
        content_type: &str,
        status: StatusCode,
        values: S,
    ) -> Result<Response, CoolError>
    where
        T: Serialize + Send + 'static,
        S: Stream<Item = Result<T, CoolError>> + Send + 'static;
}

impl<C> HttpTransport for C
where
    C: CoolCodec,
{
    fn can_encode(&self, content_type: &str) -> bool {
        media_type_matches(content_type, C::CONTENT_TYPE)
            || (content_type == CBOR_SEQUENCE_CONTENT_TYPE
                && C::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE)
    }

    fn decode_request<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: for<'de> Deserialize<'de>,
    {
        if media_type_matches(content_type, C::CONTENT_TYPE) {
            crate::codec::decode_codec_request(self, body)
        } else {
            Err(CoolError::UnsupportedMediaType(format!(
                "unsupported request Content-Type {content_type}"
            )))
        }
    }

    fn encode_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &T,
    ) -> Result<Response, CoolError>
    where
        T: Serialize + ?Sized,
    {
        if media_type_matches(content_type, C::CONTENT_TYPE) {
            encode_codec_response(self, status, value)
        } else {
            Err(CoolError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }

    fn encode_sequence_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        values: &[T],
    ) -> Result<Response, CoolError>
    where
        T: Serialize,
    {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            encode_cbor_sequence_response(self, status, values)
        } else {
            self.encode_response(content_type, status, values)
        }
    }

    fn encode_sequence_error_response(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &CoolErrorResponse,
    ) -> Result<Response, CoolError> {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            encode_cbor_sequence_response(self, status, std::slice::from_ref(value))
        } else {
            self.encode_response(content_type, status, value)
        }
    }

    fn encode_sequence_stream_response<T, S>(
        &self,
        content_type: &str,
        status: StatusCode,
        values: S,
    ) -> Result<Response, CoolError>
    where
        T: Serialize + Send + 'static,
        S: Stream<Item = Result<T, CoolError>> + Send + 'static,
    {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            encode_cbor_sequence_stream_response(self.clone(), status, values)
        } else {
            Err(CoolError::NotAcceptable(format!(
                "incremental sequence streaming requires {CBOR_SEQUENCE_CONTENT_TYPE}, got \
                 response Content-Type {content_type}"
            )))
        }
    }
}

pub(crate) struct CborCodecMarker;

impl CborCodecMarker {
    pub(crate) const CONTENT_TYPE: &'static str = "application/cbor";
}
