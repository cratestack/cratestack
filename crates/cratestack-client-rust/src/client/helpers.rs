#[cfg(feature = "codec-json")]
use cratestack_core::SelectionQuery;
use reqwest::Url;
use reqwest::header::HeaderMap;

use crate::error::ClientError;
#[cfg(feature = "codec-json")]
use crate::error::QueryPair;
use crate::runtime::wire::RuntimeHeader;

// Only called from `client/views.rs`, whose method bodies are
// themselves gated on `codec-json`.
#[cfg(feature = "codec-json")]
pub(crate) fn canonical_query_from_selection(
    selection: &SelectionQuery,
    extra_query: &[QueryPair<'_>],
) -> Result<Option<String>, ClientError> {
    let mut query: Vec<(String, String)> = Vec::new();
    if !selection.fields.is_empty() {
        query.push(("fields".to_owned(), selection.fields.join(",")));
    }
    if !selection.includes.is_empty() {
        query.push(("include".to_owned(), selection.includes.join(",")));
    }
    for (include, fields) in &selection.include_fields {
        if !fields.is_empty() {
            query.push((format!("includeFields[{include}]"), fields.join(",")));
        }
    }
    for (key, value) in extra_query {
        if *key == "fields" || *key == "include" || key.starts_with("includeFields[") {
            return Err(ClientError::BadInput(format!(
                "projection query parameter '{key}' must come from SelectionQuery, not extra_query"
            )));
        }
        query.push(((*key).to_owned(), (*value).to_owned()));
    }
    if query.is_empty() {
        return Ok(None);
    }
    serde_urlencoded::to_string(&query)
        .map(Some)
        .map_err(|error| ClientError::BadInput(format!("invalid selection query: {error}")))
}

pub(crate) fn headers_to_runtime(headers: &HeaderMap) -> Vec<RuntimeHeader> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value.to_str().ok().map(|value| RuntimeHeader {
                name: name.as_str().to_owned(),
                value: value.to_owned(),
            })
        })
        .collect()
}

pub(crate) fn build_url(
    base_url: &Url,
    path: &str,
    canonical_query: Option<&str>,
) -> Result<Url, ClientError> {
    let mut base = base_url.clone();
    if !base.path().ends_with('/') {
        let next_path = format!("{}/", base.path());
        base.set_path(&next_path);
    }
    let mut url = base.join(path.trim_start_matches('/')).map_err(|error| {
        ClientError::InvalidResponse(format!(
            "failed to resolve path '{path}' against {}: {error}",
            base
        ))
    })?;
    match canonical_query {
        Some(query) if !query.is_empty() => url.set_query(Some(query)),
        _ => url.set_query(None),
    }
    Ok(url)
}
