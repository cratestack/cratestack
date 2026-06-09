use axum::http::StatusCode;
use axum::response::Response;
use cratestack_core::{CoolCodec, CoolError, CoolErrorResponse};
use serde::{Deserialize, Serialize};

use crate::transport::{
    CBOR_SEQUENCE_CONTENT_TYPE, CborCodecMarker, HttpTransport, encode_cbor_sequence_response,
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
    Primary: CoolCodec,
    Secondary: CoolCodec,
{
    fn decode_request<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: for<'de> Deserialize<'de>,
    {
        if content_type == Primary::CONTENT_TYPE {
            self.primary.decode(body)
        } else if content_type == Secondary::CONTENT_TYPE {
            self.secondary.decode(body)
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
        if content_type == Primary::CONTENT_TYPE {
            encode_codec_response(&self.primary, status, value)
        } else if content_type == Secondary::CONTENT_TYPE {
            encode_codec_response(&self.secondary, status, value)
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
            if Primary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.primary, status, values)
            } else if Secondary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.secondary, status, values)
            } else {
                Err(CoolError::NotAcceptable(
                    "router does not have a CBOR codec for cbor-seq responses".to_owned(),
                ))
            }
        } else if content_type == Primary::CONTENT_TYPE || content_type == Secondary::CONTENT_TYPE {
            self.encode_response(content_type, status, values)
        } else {
            Err(CoolError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }

    fn encode_sequence_error_response(
        &self,
        content_type: &str,
        status: StatusCode,
        value: &CoolErrorResponse,
    ) -> Result<Response, CoolError> {
        if content_type == CBOR_SEQUENCE_CONTENT_TYPE {
            if Primary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.primary, status, std::slice::from_ref(value))
            } else if Secondary::CONTENT_TYPE == CborCodecMarker::CONTENT_TYPE {
                encode_cbor_sequence_response(&self.secondary, status, std::slice::from_ref(value))
            } else {
                Err(CoolError::NotAcceptable(
                    "router does not have a CBOR codec for cbor-seq responses".to_owned(),
                ))
            }
        } else if content_type == Primary::CONTENT_TYPE || content_type == Secondary::CONTENT_TYPE {
            self.encode_response(content_type, status, value)
        } else {
            Err(CoolError::NotAcceptable(format!(
                "no encoder configured for response Content-Type {content_type}"
            )))
        }
    }
}
