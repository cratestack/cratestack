use cratestack_core::CratestackErrorResponse;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use crate::client::TypedResponse;
use crate::codec::{
    CBOR_SEQUENCE_CONTENT_TYPE, HttpClientCodec, decode_cbor_sequence, media_type_matches,
};
use crate::error::ClientError;
use crate::runtime::wire::RuntimeResponseWire;

/// Decodes a 2xx response body to `Output`, discarding status and
/// headers. Kept byte-for-byte behaviorally identical to before #493
/// — every existing call site (`get`/`post`/`patch`/`delete`/
/// `get_view`/`list_view`/`list_view_paged`) keeps this exact
/// signature and behavior. Implemented on top of
/// [`decode_typed_response_with_metadata`] so the two paths can't
/// drift; callers that need the status/headers use that function (via
/// the `*_with_response` methods) instead of this one.
pub(crate) fn decode_typed_response<C, Output>(
    codec: &C,
    response: &RuntimeResponseWire,
) -> Result<Output, ClientError>
where
    C: HttpClientCodec,
    Output: DeserializeOwned,
{
    decode_typed_response_with_metadata(codec, response).map(|typed| typed.value)
}

/// Same decode as [`decode_typed_response`], but returns the status and
/// headers alongside the body (issue #493) — this is what makes an
/// `@version` model's `GET` → read `ETag` → `PATCH` with `If-Match`
/// round trip reachable through the typed client, since the header
/// would otherwise never survive decoding.
pub(crate) fn decode_typed_response_with_metadata<C, Output>(
    codec: &C,
    response: &RuntimeResponseWire,
) -> Result<TypedResponse<Output>, ClientError>
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
        let value = codec
            .decode_response::<Output>(content_type, &response.body)
            .map_err(ClientError::from)?;
        Ok(TypedResponse {
            value,
            status: StatusCode::from_u16(response.status_code)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            headers: response.headers.clone(),
        })
    } else {
        let error = codec
            .decode_response::<CratestackErrorResponse>(content_type, &response.body)
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

/// Build a `ClientError::Remote` from a non-2xx response, decoding the
/// body as a `CratestackErrorResponse` if possible. Used by the streaming
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
        .decode_response::<CratestackErrorResponse>(content_type, &response.body)
        .ok();
    let message = error
        .as_ref()
        .map(|value| value.message.clone())
        .unwrap_or_else(|| format!("unexpected error body for status {}", response.status_code));
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
            decode_cbor_sequence::<CratestackErrorResponse>(&response.body)
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
                .decode_response::<CratestackErrorResponse>(content_type, &response.body)
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

#[cfg(test)]
mod tests;
