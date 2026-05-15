//! URL construction + auth header application for the deployed-API
//! source.

use cratestack_migrate::table_name;
use reqwest::header::HeaderName;

use crate::data::DataError;

/// Build the list-endpoint URL for `model`: `<base>/api/<plural-snake>`.
pub(super) fn list_url(base_url: &str, model: &str) -> String {
    let plural = table_name(model);
    let trimmed = base_url.trim_end_matches('/');
    format!("{trimmed}/api/{plural}")
}

/// Build the detail-endpoint URL: `<list>/<percent-encoded pk>`.
pub(super) fn detail_url(base_url: &str, model: &str, pk: &str) -> String {
    format!("{}/{}", list_url(base_url, model), urlencoding_encode(pk))
}

/// Attach the configured auth header (if any) onto the request.
pub(super) fn apply_auth(
    builder: reqwest::RequestBuilder,
    auth: Option<&(String, String)>,
) -> reqwest::RequestBuilder {
    if let Some((name, value)) = auth
        && let Ok(parsed) = name.parse::<HeaderName>()
    {
        return builder.header(parsed, value);
    }
    builder
}

/// Tiny percent-encoder for the PK segment. Reqwest's URL builder
/// would handle this, but we build the URL by hand so the
/// `/api/{plural}/{pk}` shape stays readable in logs.
pub(super) fn urlencoding_encode(value: &str) -> String {
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

pub(super) fn cursor_to_offset(cursor: Option<&str>) -> i64 {
    cursor.and_then(|c| c.parse().ok()).unwrap_or(0)
}

/// Decode a successful JSON response into a `serde_json::Value`.
pub(super) fn decode_json(bytes: &[u8]) -> Result<serde_json::Value, DataError> {
    serde_json::from_slice(bytes).map_err(|_| DataError::Unsupported {
        what: "upstream response was not valid JSON (is this a cratestack service?)",
    })
}
