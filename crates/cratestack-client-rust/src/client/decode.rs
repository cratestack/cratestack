use cratestack_core::CoolErrorResponse;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde_json::Value as JsonValue;

use crate::codec::{
    CBOR_SEQUENCE_CONTENT_TYPE, HttpClientCodec, decode_cbor_sequence, media_type_matches,
};
use crate::error::ClientError;
use crate::runtime::wire::RuntimeResponseWire;

pub(crate) fn decode_typed_response<C, Output>(
    codec: &C,
    response: &RuntimeResponseWire,
) -> Result<Output, ClientError>
where
    C: HttpClientCodec,
    Output: DeserializeOwned,
{
    let content_type = response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
        .ok_or_else(|| {
            ClientError::InvalidResponse("response is missing Content-Type header".to_owned())
        })?;

    if (200..=299).contains(&response.status_code) {
        codec
            .decode_response::<Output>(content_type, &response.body)
            .map_err(ClientError::from)
    } else {
        let error = codec
            .decode_response::<CoolErrorResponse>(content_type, &response.body)
            .ok();
        let message = error
            .as_ref()
            .map(|value| value.message.clone())
            .unwrap_or_else(|| {
                format!("unexpected error body for status {}", response.status_code)
            });
        Err(ClientError::Remote {
            status: StatusCode::from_u16(response.status_code)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            error,
            message,
        })
    }
}

pub(crate) fn decode_json_value_response<C>(
    codec: &C,
    response: &RuntimeResponseWire,
) -> Result<JsonValue, ClientError>
where
    C: HttpClientCodec,
{
    decode_typed_response(codec, response)
}

/// Build a `ClientError::Remote` from a non-2xx response, decoding the
/// body as a `CoolErrorResponse` if possible. Used by the streaming
/// path which has a separate buffer-on-error step (success path
/// streams, error path is bounded and fits in memory).
pub(crate) fn remote_error_from_response<C>(
    codec: &C,
    response: &RuntimeResponseWire,
) -> ClientError
where
    C: HttpClientCodec,
{
    let content_type = response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
        .unwrap_or("");
    let error = codec
        .decode_response::<CoolErrorResponse>(content_type, &response.body)
        .ok();
    let message = error
        .as_ref()
        .map(|value| value.message.clone())
        .unwrap_or_else(|| {
            format!("unexpected error body for status {}", response.status_code)
        });
    ClientError::Remote {
        status: StatusCode::from_u16(response.status_code)
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        error,
        message,
    }
}

pub(crate) fn decode_sequence_response<C, Output>(
    codec: &C,
    response: &RuntimeResponseWire,
) -> Result<Vec<Output>, ClientError>
where
    C: HttpClientCodec,
    Output: DeserializeOwned,
{
    let content_type = response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
        .ok_or_else(|| {
            ClientError::InvalidResponse("response is missing Content-Type header".to_owned())
        })?;

    if (200..=299).contains(&response.status_code) {
        codec
            .decode_sequence_response::<Output>(content_type, &response.body)
            .map_err(ClientError::from)
    } else {
        let error = if media_type_matches(content_type, CBOR_SEQUENCE_CONTENT_TYPE) {
            decode_cbor_sequence::<CoolErrorResponse>(&response.body)
                .ok()
                .and_then(|mut values| {
                    if values.len() == 1 {
                        values.pop()
                    } else {
                        None
                    }
                })
        } else {
            codec
                .decode_response::<CoolErrorResponse>(content_type, &response.body)
                .ok()
        };
        let message = error
            .as_ref()
            .map(|value| value.message.clone())
            .unwrap_or_else(|| {
                format!("unexpected error body for status {}", response.status_code)
            });
        Err(ClientError::Remote {
            status: StatusCode::from_u16(response.status_code)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            error,
            message,
        })
    }
}
