use axum::http::StatusCode;
use axum::response::Response;
use cratestack_core::{CratestackCodec, CratestackError, CratestackErrorResponse};
use futures_util::Stream;
use serde::{Deserialize, Serialize};

use crate::transport::{
    CBOR_SEQUENCE_CONTENT_TYPE, CborCodecMarker, HttpTransport, encode_cbor_sequence_response,
    encode_cbor_sequence_stream_response,
};

use super::encode::encode_codec_response;

#[derive(Debug, Clone)]
pub struct CodecSet<Primary, Secondary> {
    pub(super) primary: Primary,
    pub(super) secondary: Secondary,
}

impl<Primary, Secondary> CodecSet<Primary, Secondary> {
    pub fn new(primary: Primary, secondary: Secondary) -> Self {
        Self { primary, secondary }
    }
}

impl<Primary, Secondary> HttpTransport for CodecSet<Primary, Secondary>
where
    Primary: CratestackCodec,
    Secondary: CratestackCodec,
{
    fn can_encode(&self, content_type: &str) -> bool {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            Primary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE
                || Secondary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE
        } else {
            content_type == Primary::CONTENT_TYPE || content_type == Secondary::CONTENT_TYPE
        }
    }

    fn decode_request<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CratestackError>
    where
        T: for<'de> Deserialize<'de>,
    {
        if content_type == Primary::CONTENT_TYPE {
            self.primary.decode(body)
        } else if content_type == Secondary::CONTENT_TYPE {
            self.secondary.decode(body)
        } else {
            Err(CratestackError::UnsupportedMediaType(format!(
                "unsupported request Content-Type {content_type}"
            )))
        }
    }

    fn encode_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &T,
    ) -> Result<Response, CratestackError>
    where
        T: Serialize + ?Sized,
    {
        if content_type == Primary::CONTENT_TYPE {
            encode_codec_response(&self.primary, status, value)
        } else if content_type == Secondary::CONTENT_TYPE {
            encode_codec_response(&self.secondary, status, value)
        } else {
            Err(CratestackError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }

    fn encode_sequence_response<T>(
        &self,
        content_type: &str,
        status: StatusCode,
        values: &[T],
    ) -> Result<Response, CratestackError>
    where
        T: Serialize,
    {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            if Primary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.primary, status, values)
            } else if Secondary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.secondary, status, values)
            } else {
                Err(CratestackError::NotAcceptable(
                    "router does not have a CBOR codec for cbor-seq responses".to_owned(),
                ))
            }
        } else if content_type == Primary::CONTENT_TYPE || content_type == Secondary::CONTENT_TYPE {
            self.encode_response(content_type, status, values)
        } else {
            Err(CratestackError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }

    fn encode_sequence_error_response(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &CratestackErrorResponse,
    ) -> Result<Response, CratestackError> {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            if Primary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.primary, status, std::slice::from_ref(value))
            } else if Secondary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.secondary, status, std::slice::from_ref(value))
            } else {
                Err(CratestackError::NotAcceptable(
                    "router does not have a CBOR codec for cbor-seq responses".to_owned(),
                ))
            }
        } else if content_type == Primary::CONTENT_TYPE || content_type == Secondary::CONTENT_TYPE {
            self.encode_response(content_type, status, value)
        } else {
            Err(CratestackError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }

    fn encode_sequence_stream_response<T, S>(
        &self,
        content_type: &str,
        status: StatusCode,
        values: S,
    ) -> Result<Response, CratestackError>
    where
        T: Serialize + Send + 'static,
        S: Stream<Item = Result<T, CratestackError>> + Send + 'static,
    {
        if content_type != CBOR_SEQUENCE_CONTENT_TYPE {
            return Err(CratestackError::NotAcceptable(format!(
                "incremental sequence streaming requires {CBOR_SEQUENCE_CONTENT_TYPE}, got \
                 response Content-Type {content_type}"
            )));
        }
        if Primary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
            encode_cbor_sequence_stream_response(self.primary.clone(), status, values)
        } else if Secondary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
            encode_cbor_sequence_stream_response(self.secondary.clone(), status, values)
        } else {
            Err(CratestackError::NotAcceptable(
                "router does not have a CBOR codec for cbor-seq responses".to_owned(),
            ))
        }
    }
}

#[cfg(test)]
mod tests;
