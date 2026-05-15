//! Thin fetch wrappers over the Studio backend. The UI runs on
//! `127.0.0.1:8080` in dev (Trunk) and proxies `/api/*` to the
//! backend on `7878`, so we issue same-origin requests with relative
//! URLs.

#![allow(dead_code)]

use gloo_net::http::Request;
use serde::de::DeserializeOwned;

use crate::types::{ApiError, FollowResponse, ModelList, Page, RecordResponse, SnippetResponse, TargetList};

#[derive(Debug, Clone)]
pub struct FetchError {
    pub message: String,
}

impl FetchError {
    fn from_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

async fn fetch_json<T: DeserializeOwned>(url: &str) -> Result<T, FetchError> {
    let response = Request::get(url)
        .send()
        .await
        .map_err(|e| FetchError::from_message(format!("network error: {e}")))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| FetchError::from_message(format!("body read error: {e}")))?;
    if !(200..300).contains(&status) {
        if let Ok(parsed) = serde_json::from_str::<ApiError>(&text) {
            return Err(FetchError::from_message(format!(
                "{} ({})",
                parsed.error.message, parsed.error.code
            )));
        }
        return Err(FetchError::from_message(format!(
            "HTTP {status}: {text}"
        )));
    }
    serde_json::from_str(&text)
        .map_err(|e| FetchError::from_message(format!("decode error: {e}")))
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
