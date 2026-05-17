//! Thin fetch wrappers over the Studio backend. The UI runs on
//! `127.0.0.1:8080` in dev (Trunk) and proxies `/api/*` to the
//! backend on `7878`, so we issue same-origin requests with relative
//! URLs.
//!
//! Transport plumbing lives in [`transport`]; the public endpoint
//! wrappers below build a URL and delegate.

#![allow(dead_code)]

mod transport;

use crate::types::{
    AuditResponse, DriftResponse, FollowResponse, ModelList, Page, RecordResponse,
    SearchResponse, SnippetResponse, SqlPreview, TargetList,
};

pub use transport::{FetchError, urlencode};

use transport::{fetch_json, send_empty, send_json};

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

pub async fn preview_sql(
    target: &str,
    model: &str,
    op: &str,
    pk: Option<&str>,
) -> Result<SqlPreview, FetchError> {
    let mut url = format!("/api/targets/{target}/models/{model}/sql?op={op}");
    if let Some(p) = pk {
        url.push_str(&format!("&pk={}", urlencode(p)));
    }
    fetch_json(&url).await
}

pub async fn target_drift(target: &str) -> Result<DriftResponse, FetchError> {
    fetch_json(&format!("/api/targets/{target}/drift")).await
}

pub async fn schema_search(target: &str, query: &str) -> Result<SearchResponse, FetchError> {
    fetch_json(&format!(
        "/api/targets/{target}/search?q={}",
        urlencode(query)
    ))
    .await
}

pub async fn audit_log(limit: u32) -> Result<AuditResponse, FetchError> {
    fetch_json(&format!("/api/audit?limit={limit}")).await
}
