//! Thin fetch wrappers over the Studio backend. The UI runs on
//! `127.0.0.1:8080` in dev (Trunk) and proxies `/api/*` to the
//! backend on `7878`, so we issue same-origin requests with relative
//! URLs.

#![allow(dead_code)]

use gloo_net::http::Request;
use serde::de::DeserializeOwned;

use crate::types::{ApiError, FieldError, FollowResponse, ModelList, Page, RecordResponse, SnippetResponse, TargetList};

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
    fn from_message(message: impl Into<String>) -> Self {
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

async fn fetch_json<T: DeserializeOwned>(url: &str) -> Result<T, FetchError> {
    let response = Request::get(url)
        .send()
        .await
        .map_err(|e| FetchError::from_message(format!("network error: {e}")))?;
    finish_response(response).await
}

async fn send_json<B, T>(method: &str, url: &str, body: &B) -> Result<T, FetchError>
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

async fn send_empty<T>(method: &str, url: &str) -> Result<T, FetchError>
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

pub async fn list_targets() -> Result<TargetList, FetchError> {
    fetch_json("/api/targets").await
}

pub async fn list_models(target: &str) -> Result<ModelList, FetchError> {
    fetch_json(&format!("/api/targets/{target}/models")).await
}

pub async fn list_records(
    target: &str,
    model: &str,
    cursor: Option<&str>,
    limit: u32,
) -> Result<Page, FetchError> {
    let mut url = format!("/api/targets/{target}/models/{model}/records?limit={limit}");
    if let Some(c) = cursor {
        url.push_str(&format!("&cursor={}", urlencode(c)));
    }
    fetch_json(&url).await
}

pub async fn get_record(
    target: &str,
    model: &str,
    pk: &str,
) -> Result<RecordResponse, FetchError> {
    fetch_json(&format!(
        "/api/targets/{target}/models/{model}/records/{}",
        urlencode(pk)
    ))
    .await
}

pub async fn follow_relation(
    target: &str,
    model: &str,
    pk: &str,
    field: &str,
) -> Result<FollowResponse, FetchError> {
    fetch_json(&format!(
        "/api/targets/{target}/models/{model}/records/{}/rel/{}",
        urlencode(pk),
        urlencode(field)
    ))
    .await
}

pub async fn snippet(
    target: &str,
    model: &str,
    pk: &str,
) -> Result<SnippetResponse, FetchError> {
    fetch_json(&format!(
        "/api/targets/{target}/models/{model}/snippet?pk={}",
        urlencode(pk)
    ))
    .await
}

pub async fn create_record(
    target: &str,
    model: &str,
    payload: &serde_json::Value,
) -> Result<RecordResponse, FetchError> {
    send_json(
        "POST",
        &format!("/api/targets/{target}/models/{model}/records"),
        payload,
    )
    .await
}

pub async fn update_record(
    target: &str,
    model: &str,
    pk: &str,
    payload: &serde_json::Value,
) -> Result<RecordResponse, FetchError> {
    send_json(
        "PATCH",
        &format!(
            "/api/targets/{target}/models/{model}/records/{}",
            urlencode(pk)
        ),
        payload,
    )
    .await
}

pub async fn delete_record(
    target: &str,
    model: &str,
    pk: &str,
) -> Result<RecordResponse, FetchError> {
    send_empty(
        "DELETE",
        &format!(
            "/api/targets/{target}/models/{model}/records/{}",
            urlencode(pk)
        ),
    )
    .await
}

fn urlencode(value: &str) -> String {
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
