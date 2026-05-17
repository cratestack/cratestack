//! Low-level fetch helpers shared by every endpoint wrapper.

use gloo_net::http::Request;
use serde::de::DeserializeOwned;

use crate::types::{ApiError, FieldError};

/// A failed fetch. When the server returned a structured envelope,
/// `code` carries the `error.code` value; otherwise `code` is empty
/// and `message` describes the network-level failure.
#[derive(Debug, Clone, Default)]
pub struct FetchError {
    pub message: String,
    pub code: String,
    pub fields: Vec<FieldError>,
}

impl FetchError {
    pub(super) fn from_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Self::default()
        }
    }
    fn from_envelope(api: ApiError) -> Self {
        Self {
            message: api.error.message,
            code: api.error.code,
            fields: api.error.fields,
        }
    }
}

async fn finish_response<T: DeserializeOwned>(
    response: gloo_net::http::Response,
) -> Result<T, FetchError> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| FetchError::from_message(format!("body read error: {e}")))?;
    if !(200..300).contains(&status) {
        if let Ok(parsed) = serde_json::from_str::<ApiError>(&text) {
            return Err(FetchError::from_envelope(parsed));
        }
        return Err(FetchError::from_message(format!("HTTP {status}: {text}")));
    }
    if text.is_empty() {
        return serde_json::from_str("null")
            .map_err(|e| FetchError::from_message(format!("decode error: {e}")));
    }
    serde_json::from_str(&text)
        .map_err(|e| FetchError::from_message(format!("decode error: {e}")))
}

pub(super) async fn fetch_json<T: DeserializeOwned>(url: &str) -> Result<T, FetchError> {
    let response = Request::get(url)
        .send()
        .await
        .map_err(|e| FetchError::from_message(format!("network error: {e}")))?;
    finish_response(response).await
}

pub(super) async fn send_json<B, T>(method: &str, url: &str, body: &B) -> Result<T, FetchError>
where
    B: serde::Serialize,
    T: DeserializeOwned,
{
    let json = serde_json::to_string(body)
        .map_err(|e| FetchError::from_message(format!("encode error: {e}")))?;
    let builder = match method {
        "POST" => Request::post(url),
        "PATCH" => Request::patch(url),
        "PUT" => Request::put(url),
        other => panic!("unsupported method: {other}"),
    };
    let response = builder
        .header("content-type", "application/json")
        .body(json)
        .map_err(|e| FetchError::from_message(format!("request build error: {e}")))?
        .send()
        .await
        .map_err(|e| FetchError::from_message(format!("network error: {e}")))?;
    finish_response(response).await
}

pub(super) async fn send_empty<T>(method: &str, url: &str) -> Result<T, FetchError>
where
    T: DeserializeOwned,
{
    let builder = match method {
        "DELETE" => Request::delete(url),
        other => panic!("unsupported method: {other}"),
    };
    let response = builder
        .send()
        .await
        .map_err(|e| FetchError::from_message(format!("network error: {e}")))?;
    finish_response(response).await
}

pub fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}
