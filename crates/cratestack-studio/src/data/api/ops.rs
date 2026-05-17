//! CRUD operations for the deployed-API source. Each function builds
//! a request via [`super::transport`] helpers and decodes the response
//! into a [`Row`] or [`Page`].

use reqwest::{Client, StatusCode};

use crate::data::{DataError, Page, PageRequest, Row};

use super::transport::{apply_auth, cursor_to_offset, decode_json, detail_url, list_url};

type Auth = Option<(String, String)>;

pub(super) async fn list(
    client: &Client,
    base_url: &str,
    auth: &Auth,
    model: &str,
    page: PageRequest<'_>,
) -> Result<Page, DataError> {
    let url = list_url(base_url, model);
    let offset = cursor_to_offset(page.cursor);
    let limit = page.limit.unwrap_or(50).clamp(1, 500) as i64;

    let builder = client
        .get(&url)
        .query(&[("limit", limit.to_string()), ("offset", offset.to_string())]);
    let response = apply_auth(builder, auth.as_ref()).send().await?;

    if response.status() == StatusCode::NOT_FOUND {
        return Err(DataError::UnknownModel {
            model: model.to_owned(),
        });
    }
    let bytes = response.error_for_status()?.bytes().await?;
    let value = decode_json(&bytes)?;

    let rows: Vec<Row> = value
        .get("items")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_object().cloned())
                .collect()
        })
        .unwrap_or_default();

    let has_next = value
        .get("pageInfo")
        .and_then(|p| p.get("hasNextPage"))
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    let next_cursor = has_next.then(|| (offset + rows.len() as i64).to_string());
    Ok(Page { rows, next_cursor })
}

pub(super) async fn get(
    client: &Client,
    base_url: &str,
    auth: &Auth,
    model: &str,
    pk: &str,
) -> Result<Option<Row>, DataError> {
    let url = detail_url(base_url, model, pk);
    let response = apply_auth(client.get(&url), auth.as_ref()).send().await?;

    match response.status() {
        StatusCode::NOT_FOUND => Ok(None),
        status if status.is_success() => {
            let bytes = response.bytes().await?;
            let value = decode_json(&bytes)?;
            Ok(match value {
                serde_json::Value::Object(map) => Some(map),
                _ => None,
            })
        }
        _ => Err(DataError::Api(response.error_for_status().unwrap_err())),
    }
}

pub(super) async fn create(
    client: &Client,
    base_url: &str,
    auth: &Auth,
    model: &str,
    payload: &Row,
) -> Result<Row, DataError> {
    let url = list_url(base_url, model);
    let body =
        serde_json::to_vec(&serde_json::Value::Object(payload.clone())).expect("json serialize");
    let builder = client
        .post(&url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body);
    let response = apply_auth(builder, auth.as_ref()).send().await?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(DataError::UnknownModel {
            model: model.to_owned(),
        });
    }
    let bytes = response.error_for_status()?.bytes().await?;
    let value = decode_json(&bytes)?;
    match value {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(DataError::Unsupported {
            what: "upstream create response was not a JSON object",
        }),
    }
}

pub(super) async fn update(
    client: &Client,
    base_url: &str,
    auth: &Auth,
    model: &str,
    pk: &str,
    payload: &Row,
) -> Result<Option<Row>, DataError> {
    let url = detail_url(base_url, model, pk);
    let body =
        serde_json::to_vec(&serde_json::Value::Object(payload.clone())).expect("json serialize");
    let builder = client
        .patch(&url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body);
    let response = apply_auth(builder, auth.as_ref()).send().await?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let bytes = response.error_for_status()?.bytes().await?;
    let value = decode_json(&bytes)?;
    Ok(match value {
        serde_json::Value::Object(map) => Some(map),
        _ => None,
    })
}

pub(super) async fn delete(
    client: &Client,
    base_url: &str,
    auth: &Auth,
    model: &str,
    pk: &str,
) -> Result<Option<Row>, DataError> {
    let url = detail_url(base_url, model, pk);
    let response = apply_auth(client.delete(&url), auth.as_ref())
        .send()
        .await?;
    match response.status() {
        StatusCode::NOT_FOUND => Ok(None),
        StatusCode::NO_CONTENT => Ok(Some(Row::new())),
        status if status.is_success() => {
            let bytes = response.bytes().await?;
            let value = decode_json(&bytes)?;
            Ok(match value {
                serde_json::Value::Object(map) => Some(map),
                _ => Some(Row::new()),
            })
        }
        _ => Err(DataError::Api(response.error_for_status().unwrap_err())),
    }
}
