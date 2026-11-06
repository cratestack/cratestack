//! Opening a compact request body.

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use cratestack_codec_cbor::compact::cbor_from_compact;
use cratestack_codec_json::compact::json_from_compact;
use cratestack_core::compact::{COMPACT_CBOR, Direction, OpKeys, plain_media_type_for};
use cratestack_core::{
    CratestackError, PAYLOAD_TYPE_UNSUPPORTED_CODE, PAYLOAD_TYPE_UNSUPPORTED_REST_CODE,
};

use super::layer::CompactLayer;
use super::negotiate::is_unary;
use crate::middleware_error::{middleware_coded_response, middleware_error_response_with_status};

/// Rewrite the compact body of `req` back to names and its `Content-Type`
/// to the plain type, or the refusal to answer with: `415` when the
/// selector names no known contract for the op, `413` over the limit,
/// the codec's own status for a malformed body.
pub(super) async fn open(
    layer: &CompactLayer,
    keys: Option<&OpKeys>,
    compact: &str,
    req: Request,
) -> Result<Request, Box<Response>> {
    let (mut parts, body) = req.into_parts();
    let path = parts.uri.path().to_owned();
    let (Some(keys), true) = (keys, is_unary(compact)) else {
        return Err(Box::new(unsupported(&parts.headers, &path)));
    };
    let bytes = axum::body::to_bytes(body, layer.body_limit)
        .await
        .map_err(|_| Box::new(too_large(&parts.headers, &path)))?;
    let named = if bytes.is_empty() {
        Vec::new()
    } else {
        let node = layer.tables.node(keys, Direction::Request);
        let opened = if compact == COMPACT_CBOR {
            cbor_from_compact(&layer.tables, node, &bytes)
        } else {
            json_from_compact(&layer.tables, node, &bytes)
        };
        opened.map_err(|error| Box::new(refused(&parts.headers, &path, error)))?
    };
    if named.len() > layer.body_limit {
        return Err(Box::new(too_large(&parts.headers, &path)));
    }
    let plain = plain_media_type_for(compact).unwrap_or("application/cbor");
    parts
        .headers
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(plain));
    parts.headers.remove(header::CONTENT_ENCODING);
    parts.headers.remove(header::TRANSFER_ENCODING);
    parts
        .headers
        .insert(header::CONTENT_LENGTH, HeaderValue::from(named.len()));
    Ok(Request::from_parts(parts, Body::from(Bytes::from(named))))
}

fn unsupported(headers: &axum::http::HeaderMap, path: &str) -> Response {
    middleware_coded_response(
        headers,
        path,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        (
            PAYLOAD_TYPE_UNSUPPORTED_CODE,
            PAYLOAD_TYPE_UNSUPPORTED_REST_CODE,
        ),
        "the payload type of this request is not accepted for this operation",
    )
}

fn too_large(headers: &axum::http::HeaderMap, path: &str) -> Response {
    middleware_error_response_with_status(
        headers,
        path,
        StatusCode::PAYLOAD_TOO_LARGE,
        CratestackError::BadRequest("request body exceeds the size limit".to_owned()),
    )
}

fn refused(headers: &axum::http::HeaderMap, path: &str, error: CratestackError) -> Response {
    let error = CratestackError::BadRequest(
        error
            .detail()
            .unwrap_or("malformed compact body")
            .to_owned(),
    );
    middleware_error_response_with_status(headers, path, StatusCode::BAD_REQUEST, error)
}
