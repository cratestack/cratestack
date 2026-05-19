use cratestack_codec_cbor::CborCodec;
#[cfg(feature = "codec-json")]
use cratestack_codec_json::JsonCodec;
use cratestack_core::{CoolCodec, CoolError};
use serde::de::DeserializeOwned;

pub(crate) const CBOR_SEQUENCE_CONTENT_TYPE: &str = "application/cbor-seq";

pub trait HttpClientCodec: CoolCodec {
    fn accept_header_value(&self) -> &'static str;

    fn sequence_accept_header_value(&self) -> &'static str;

    fn decode_response<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: DeserializeOwned;

    fn decode_sequence_response<T>(
        &self,
        content_type: &str,
        body: &[u8],
    ) -> Result<Vec<T>, CoolError>
    where
        T: DeserializeOwned;
}

impl HttpClientCodec for CborCodec {
    fn accept_header_value(&self) -> &'static str {
        // With `codec-json` disabled the client cannot actually
        // decode a JSON response — drop it from the Accept header
        // so a server with content negotiation chooses CBOR (or
        // fails on no acceptable representation) rather than
        // sending JSON the client will then error on.
        #[cfg(feature = "codec-json")]
        {
            "application/cbor, application/json"
        }
        #[cfg(not(feature = "codec-json"))]
        {
            "application/cbor"
        }
    }

    fn sequence_accept_header_value(&self) -> &'static str {
        #[cfg(feature = "codec-json")]
        {
            "application/cbor-seq, application/cbor, application/json"
        }
        #[cfg(not(feature = "codec-json"))]
        {
            "application/cbor-seq, application/cbor"
        }
    }

    fn decode_response<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: DeserializeOwned,
    {
        if media_type_matches(content_type, CborCodec::CONTENT_TYPE) {
            self.decode(body)
        } else {
            #[cfg(feature = "codec-json")]
            if media_type_matches(content_type, JsonCodec::CONTENT_TYPE) {
                return JsonCodec.decode(body);
            }
            Err(CoolError::Codec(format!(
                "unsupported response Content-Type {content_type}"
            )))
        }
    }

    fn decode_sequence_response<T>(
        &self,
        content_type: &str,
        body: &[u8],
    ) -> Result<Vec<T>, CoolError>
    where
        T: DeserializeOwned,
    {
        if media_type_matches(content_type, CBOR_SEQUENCE_CONTENT_TYPE) {
            decode_cbor_sequence(body)
        } else if media_type_matches(content_type, CborCodec::CONTENT_TYPE) {
            self.decode(body)
        } else {
            #[cfg(feature = "codec-json")]
            if media_type_matches(content_type, JsonCodec::CONTENT_TYPE) {
                return JsonCodec.decode(body);
            }
            Err(CoolError::Codec(format!(
                "unsupported response Content-Type {content_type}"
            )))
        }
    }
}

#[cfg(feature = "codec-json")]
impl HttpClientCodec for JsonCodec {
    fn accept_header_value(&self) -> &'static str {
        "application/json, application/cbor"
    }

    fn sequence_accept_header_value(&self) -> &'static str {
        "application/cbor-seq, application/json, application/cbor"
    }

    fn decode_response<T>(&self, content_type: &str, body: &[u8]) -> Result<T, CoolError>
    where
        T: DeserializeOwned,
    {
        if media_type_matches(content_type, JsonCodec::CONTENT_TYPE) {
            self.decode(body)
        } else if media_type_matches(content_type, CborCodec::CONTENT_TYPE) {
            CborCodec.decode(body)
        } else {
            Err(CoolError::Codec(format!(
                "unsupported response Content-Type {content_type}"
            )))
        }
    }

    fn decode_sequence_response<T>(
        &self,
        content_type: &str,
        body: &[u8],
    ) -> Result<Vec<T>, CoolError>
    where
        T: DeserializeOwned,
    {
        if media_type_matches(content_type, CBOR_SEQUENCE_CONTENT_TYPE) {
            decode_cbor_sequence(body)
        } else if media_type_matches(content_type, JsonCodec::CONTENT_TYPE) {
            self.decode(body)
        } else if media_type_matches(content_type, CborCodec::CONTENT_TYPE) {
            CborCodec.decode(body)
        } else {
            Err(CoolError::Codec(format!(
                "unsupported response Content-Type {content_type}"
            )))
        }
    }
}

pub(crate) fn media_type_matches(candidate: &str, expected: &str) -> bool {
    candidate.split(';').next().unwrap_or(candidate).trim() == expected
}

pub(crate) fn decode_cbor_sequence<T>(bytes: &[u8]) -> Result<Vec<T>, CoolError>
where
    T: DeserializeOwned,
{
    let mut values = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let mut deserializer = minicbor_serde::Deserializer::new(&bytes[offset..]);
        values.push(T::deserialize(&mut deserializer).map_err(|error| {
            CoolError::Codec(format!("failed to decode CBOR sequence body: {error}"))
        })?);
        let consumed = deserializer.decoder().position();
        if consumed == 0 {
            return Err(CoolError::Codec(
                "failed to decode CBOR sequence body: decoder made no progress".to_owned(),
            ));
        }
        offset += consumed;
    }
    Ok(values)
}
