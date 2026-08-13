//! Minimal JSON/CBOR content negotiation for [`crate::axum_handler`].
//!
//! The downstream crate this was absorbed from pulled a sibling `error-kit`
//! crate in for this — a response envelope with a `meta: BTreeMap<String,
//! String>` field and COSE-body support, neither of which this crate's two
//! handlers ever populate. Depending on an app-level HTTP-response-shape
//! crate is exactly the kind of downstream-specific coupling absorption is
//! supposed to drop rather than port wholesale, so this module hand-rolls
//! only the subset actually used: decode a request body as JSON or CBOR
//! based on `Content-Type`, and encode a response body as JSON or CBOR
//! based on `Accept` — defaulting to CBOR to match
//! [`crate::axum_handler::decode_body`]'s own request-side default.

use axum::http::{HeaderMap, StatusCode, header::ACCEPT};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Decode a request body as JSON or CBOR based on the `Content-Type`
/// header. An empty body yields `T::default()`. Exposed at the crate root
/// ([`crate::decode_body`]) for services writing their own internal
/// callback handlers against the same wire contract.
pub fn decode_body<T: DeserializeOwned + Default>(
    headers: &HeaderMap,
    body: &[u8],
) -> Result<T, String> {
    if body.is_empty() {
        return Ok(T::default());
    }
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or("").trim().to_owned())
        .unwrap_or_else(|| "application/cbor".to_owned());
    match content_type.as_str() {
        "application/json" => serde_json::from_slice(body).map_err(|err| err.to_string()),
        "application/cbor" | "application/cbor-seq" | "" => {
            minicbor_serde::from_slice(body).map_err(|err| err.to_string())
        }
        other => Err(format!("unsupported content type: {other}")),
    }
}

/// `true` when the caller's `Accept` header asks for JSON specifically.
/// Every other value (including no `Accept` header at all) negotiates to
/// CBOR, matching [`decode_body`]'s own default.
fn wants_json(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT)
        .and_then(|value| value.to_str().ok())
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(|value| value.split(';').next().unwrap_or("").trim())
        .any(|media_type| media_type == "application/json")
}

/// Encode `data` as the response body, JSON or CBOR per [`wants_json`], at
/// the given status code.
pub(crate) fn respond<T: Serialize>(headers: &HeaderMap, status: StatusCode, data: &T) -> Response {
    if wants_json(headers) {
        match serde_json::to_vec(data) {
            Ok(body) => build(status, "application/json", body),
            Err(error) => serialization_failure(error.to_string()),
        }
    } else {
        match minicbor_serde::to_vec(data) {
            Ok(body) => build(status, "application/cbor", body),
            Err(error) => serialization_failure(error.to_string()),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}

/// Build an error response in the same negotiated JSON/CBOR shape as
/// [`respond`], with a small `{code, message}` body.
pub(crate) fn respond_error(
    headers: &HeaderMap,
    status: StatusCode,
    code: &str,
    message: &str,
) -> Response {
    respond(headers, status, &ErrorBody { code, message })
}

fn build(status: StatusCode, content_type: &'static str, body: Vec<u8>) -> Response {
    (
        status,
        [(axum::http::header::CONTENT_TYPE, content_type)],
        body,
    )
        .into_response()
}

fn serialization_failure(detail: String) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::json!({"code": "serialization_failed", "message": detail}).to_string(),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn decode_body_empty_yields_default() {
        let headers = HeaderMap::new();
        let value: crate::DrainRequest = decode_body(&headers, &[]).expect("default");
        assert_eq!(value, crate::DrainRequest::default());
    }

    #[test]
    fn decode_body_json_by_content_type() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        let body = serde_json::to_vec(&serde_json::json!({"max": 7})).unwrap();
        let value: crate::DrainRequest = decode_body(&headers, &body).expect("decode json");
        assert_eq!(value.max, 7);
    }

    #[test]
    fn decode_body_cbor_is_the_default() {
        let headers = HeaderMap::new();
        let body = minicbor_serde::to_vec(serde_json::json!({"max": 9})).unwrap();
        let value: crate::DrainRequest = decode_body(&headers, &body).expect("decode cbor");
        assert_eq!(value.max, 9);
    }

    #[test]
    fn decode_body_rejects_unknown_content_type() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain"),
        );
        let result: Result<crate::DrainRequest, _> = decode_body(&headers, b"nope");
        assert!(result.is_err());
    }

    #[test]
    fn wants_json_true_only_for_explicit_json_accept() {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        assert!(wants_json(&headers));

        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/cbor"));
        assert!(!wants_json(&headers));

        assert!(!wants_json(&HeaderMap::new()));
    }
}
