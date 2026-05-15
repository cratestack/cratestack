//! `GET /api/targets/:key/models/:model/export?format=csv|json&limit=N`
//!
//! Dumps up to `limit` rows of `model` in the requested format. We
//! iterate the source's cursor pagination internally so the caller
//! gets one body rather than having to stitch pages together. The
//! cap is bounded ([`EXPORT_CAP`]) to keep memory predictable; the
//! intent is "developer pulling a sample for a notebook," not "ETL."

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use cratestack_migrate::table_name;
use serde::Deserialize;

use crate::api::ApiError;
use crate::data::{PageRequest, Row};
use crate::workspace::LoadedWorkspace;

/// Hard upper bound on the number of rows the export endpoint will
/// pull. Same shape as the list endpoint's `MAX_PAGE_LIMIT` but a few
/// orders of magnitude higher so a notebook-sized sample fits in one
/// call.
pub const EXPORT_CAP: u32 = 10_000;

#[derive(Debug, Deserialize, Default)]
pub struct ExportQuery {
    pub format: Option<ExportFormat>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Csv,
    Json,
}

pub async fn export_records(
    State(state): State<Arc<LoadedWorkspace>>,
    Path((key, model)): Path<(String, String)>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let target = state
        .target(&key)
        .ok_or_else(|| ApiError::UnknownTarget(key.clone()))?;
    let format = q.format.unwrap_or(ExportFormat::Json);
    let mut remaining = q.limit.unwrap_or(EXPORT_CAP).min(EXPORT_CAP);
    let mut all_rows: Vec<Row> = Vec::new();
    let mut cursor: Option<String> = None;

    while remaining > 0 {
        let page_limit = remaining.min(500);
        let page = target
            .source
            .list(
                &model,
                PageRequest {
                    cursor: cursor.as_deref(),
                    limit: Some(page_limit),
                },
            )
            .await?;
        let count = page.rows.len() as u32;
        all_rows.extend(page.rows);
        match page.next_cursor {
            Some(next) if count == page_limit => {
                cursor = Some(next);
                remaining = remaining.saturating_sub(count);
            }
            _ => break,
        }
    }

    let filename = format!("{}-{}", target.key, table_name(&model));
    let (body, ctype, ext) = match format {
        ExportFormat::Json => (
            serde_json::to_vec(&all_rows).unwrap_or_else(|_| Vec::from(b"[]" as &[u8])),
            "application/json",
            "json",
        ),
        ExportFormat::Csv => (render_csv(&all_rows), "text/csv", "csv"),
    };

    let disposition = format!("attachment; filename=\"{filename}.{ext}\"");
    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, ctype)
        .body(Body::from(body))
        .expect("response builds");
    resp.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).expect("disposition is ascii"),
    );
    Ok(resp.into_response())
}

/// CSV writer that handles the only escape SQL data realistically
/// needs: doubling embedded `"` characters and wrapping any cell that
/// contains a comma / quote / newline in quotes. Header is the union
/// of keys in declaration order of the first row, then any new keys
/// appended.
pub(crate) fn render_csv(rows: &[Row]) -> Vec<u8> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mut headers: Vec<String> = Vec::new();
    for row in rows {
        for key in row.keys() {
            if !headers.iter().any(|h| h == key) {
                headers.push(key.clone());
            }
        }
    }
    let mut out = String::new();
    for (i, h) in headers.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&escape_cell(h));
    }
    out.push('\n');
    for row in rows {
        for (i, h) in headers.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let cell = match row.get(h) {
                None | Some(serde_json::Value::Null) => String::new(),
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
            };
            out.push_str(&escape_cell(&cell));
        }
        out.push('\n');
    }
    out.into_bytes()
}

fn escape_cell(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        let escaped = value.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_csv_with_header_union_and_escapes_quotes() {
        let rows: Vec<Row> = vec![
            json!({ "id": "p1", "title": "first" }).as_object().unwrap().clone(),
            json!({ "id": "p2", "title": "with \"quotes\"" }).as_object().unwrap().clone(),
        ];
        let bytes = render_csv(&rows);
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.starts_with("id,title\n"), "{text}");
        assert!(text.contains(r#""with ""quotes""""#), "{text}");
    }

    #[test]
    fn empty_rows_yields_empty_csv() {
        assert!(render_csv(&[]).is_empty());
    }
}
