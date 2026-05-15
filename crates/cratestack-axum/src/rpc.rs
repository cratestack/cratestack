//! Runtime primitives for the `transport rpc` generation style.
//!
//! See `docs/design/rpc-transport.md` for the full design. This module
//! provides the binding-side surface that schemas with `transport rpc`
//! generate against:
//!
//! - `POST /rpc/{op_id}` — unary calls. Body is the codec-encoded *input*
//!   (no frame wrapper); response body is the codec-encoded *output* on
//!   success, or an [`RpcErrorBody`] on error with HTTP status mapped via
//!   [`CoolError::status_code`].
//! - `POST /rpc/batch` — sequence of `RpcRequest` frames in, sequence of
//!   `RpcResponseFrame` frames out in the same order. Per-frame errors
//!   don't poison the batch.
//!
//! Subscriptions and streaming live on WebSocket and `application/cbor-seq`
//! respectively; they are deferred to a follow-up patch.
//!
//! The macro emits the dispatch table and the `rpc_router` constructor.
//! This crate provides the shared frame shapes, error mapping, and the
//! `RPC_*_PATH` constants both sides agree on.

use axum::http::HeaderMap;
use cratestack_core::CoolError;
use serde::{Deserialize, Serialize};

use crate::HttpTransport;

// Re-export the wire shapes from `cratestack-core::rpc`. Both the server
// binding and every generated client agree on those shapes, and lifting
// them into core means the client crates don't need to depend on axum.
pub use cratestack_core::rpc::{
    RPC_BATCH_PATH, RPC_UNARY_PATH, RpcErrorBody, RpcRequest, RpcResponseFrame,
    cool_error_code_to_rpc_code, rpc_code,
};

/// Codec/transport capabilities for every RPC binding route. Both unary
/// and batch accept and emit CBOR or JSON, default CBOR; sequence
/// responses (streaming) are not yet supported by this binding.
///
/// Used by `encode_transport_result_with_status_for` to negotiate
/// response content type when the dispatcher synthesizes an error
/// response or wraps a batch result.
pub const RPC_BINDING_CAPABILITIES: cratestack_core::RouteTransportCapabilities =
    cratestack_core::RouteTransportCapabilities {
        request_types: &["application/cbor", "application/json"],
        response_types: &["application/cbor", "application/json"],
        default_response_type: "application/cbor",
        supports_sequence_response: false,
    };

// -----------------------------------------------------------------------------
// CRUD input shapes
//
// The RPC binding wraps each model verb's input in a stable, model-agnostic
// shape. The macro decodes the body into one of these, then reconstructs
// whatever axum extractor the existing CRUD handler expects (`Path(id)`,
// `RawQuery(...)`, `Bytes`) and delegates. The handlers themselves are
// untouched.
//
// The list shape mirrors the REST URL query 1:1 — same keys, same semantics —
// so REST clients can migrate to RPC without re-learning the filter / order /
// pagination vocabulary. Synthesis back to a URL query happens in
// [`synthesize_list_query`]; the existing list handler parses it via
// `parse_model_list_query`.
// -----------------------------------------------------------------------------

/// RPC input for `model.<X>.get` and `model.<X>.delete`. The PK type is
/// instantiated per-model at the macro emission site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcPkInput<Pk> {
    pub id: Pk,
}

/// RPC input for `model.<X>.update`. Parameterized on both the PK type
/// and the model's concrete `Update<X>Input` so the patch decodes
/// straight to its real type — round-tripping through
/// `serde_json::Value` would corrupt CBOR `Option::None` values (which
/// `minicbor-serde` encodes as `0xf6` simple-null but `serde_json::Value`
/// encodes as the CBOR empty-array marker; see comments in
/// `cratestack-codec-cbor`). The dispatcher re-encodes `patch` through
/// the same codec before handing it to the existing update handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcUpdateInput<Pk, Patch> {
    pub id: Pk,
    pub patch: Patch,
}

/// Single arbitrary key/value predicate inside [`RpcListInput::filters`].
/// Models the REST URL form's "anything that isn't a reserved keyword is a
/// predicate" rule (e.g. `?published=true&authorId=42`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcListPredicate {
    pub key: String,
    pub value: String,
}

/// RPC input for `model.<X>.list`. Mirrors the REST URL query 1:1 — every
/// optional field maps to a query param of the same name, predicates carry
/// arbitrary `(key, value)` pairs that aren't reserved keywords.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RpcListInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    /// Selection fields (`?fields=a,b,c`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    /// Included relations (`?include=author,comments`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    /// Fields per included relation (`?includeFields[author]=id,name`).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub include_fields: std::collections::BTreeMap<String, Vec<String>>,
    /// Order expression (`?sort=name asc`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    /// Top-level filter expression (`?where=...`).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "where")]
    pub where_expr: Option<String>,
    /// Disjunction filter (`?or=...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub or: Option<String>,
    /// Arbitrary `key=value` predicates (anything not in the reserved set).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<RpcListPredicate>,
}

/// Synthesize a URL query string from an [`RpcListInput`] in exactly the
/// shape the macro-generated `parse_model_list_query` parses. Returns
/// `None` when the input has no fields set (the existing handler treats a
/// missing query the same as an empty one — no point allocating).
pub fn synthesize_list_query(input: &RpcListInput) -> Option<String> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(limit) = input.limit {
        pairs.push(("limit".to_owned(), limit.to_string()));
    }
    if let Some(offset) = input.offset {
        pairs.push(("offset".to_owned(), offset.to_string()));
    }
    if let Some(fields) = &input.fields {
        pairs.push(("fields".to_owned(), fields.join(",")));
    }
    if let Some(include) = &input.include {
        pairs.push(("include".to_owned(), include.join(",")));
    }
    for (relation, fields) in &input.include_fields {
        pairs.push((format!("includeFields[{relation}]"), fields.join(",")));
    }
    if let Some(sort) = &input.sort {
        pairs.push(("sort".to_owned(), sort.clone()));
    }
    if let Some(where_expr) = &input.where_expr {
        pairs.push(("where".to_owned(), where_expr.clone()));
    }
    if let Some(or) = &input.or {
        pairs.push(("or".to_owned(), or.clone()));
    }
    for predicate in &input.filters {
        pairs.push((predicate.key.clone(), predicate.value.clone()));
    }

    if pairs.is_empty() {
        return None;
    }

    Some(
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .finish(),
    )
}

// -----------------------------------------------------------------------------
// Error encoding + handler-response post-processing
//
// Every error that exits the RPC binding ends up wire-shaped as
// `RpcErrorBody { code, message, details? }` with gRPC-style lowercase
// codes — regardless of whether it originated inside the dispatcher
// (before reaching a handler) or inside the handler itself. The two
// helpers below cover both paths.
// -----------------------------------------------------------------------------

/// Build an `axum::Response` carrying an [`RpcErrorBody`] for a
/// [`CoolError`] raised inside the dispatcher (e.g. body decode
/// failure, unknown op id). The HTTP status comes from
/// [`CoolError::status_code`]; the body is codec-encoded via the
/// request's codec, content-type negotiated against
/// [`RPC_BINDING_CAPABILITIES`].
pub fn encode_rpc_error<C>(codec: &C, headers: &HeaderMap, error: &CoolError) -> axum::response::Response
where
    C: HttpTransport,
{
    let body = RpcErrorBody::from_cool(error);
    let status = error.status_code();
    encode_rpc_value_response(codec, headers, status, body)
}

/// Post-process a handler-emitted response. Success responses pass
/// through unchanged. Non-2xx responses are buffered, their bodies
/// decoded as [`cratestack_core::CoolErrorResponse`] (the REST shape
/// the existing axum handlers emit), translated to [`RpcErrorBody`]
/// with the gRPC-style code, and re-encoded with the same HTTP status.
///
/// Called once per dispatch (inside `rpc_dispatch_inner`) so unary and
/// batch both see uniformly RpcErrorBody-shaped error bodies.
pub async fn convert_handler_error_response<C>(
    response: axum::response::Response,
    codec: &C,
    headers: &HeaderMap,
) -> axum::response::Response
where
    C: HttpTransport,
{
    if response.status().is_success() {
        return response;
    }

    let status = response.status();
    let body_bytes = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
        Ok(bytes) => bytes.to_vec(),
        Err(error) => {
            // Buffering failed — synthesize an internal error frame.
            let cool = CoolError::Internal(format!("buffer handler error body: {error}"));
            return encode_rpc_error(codec, headers, &cool);
        }
    };

    let rpc_body = match decode_rpc_body::<_, cratestack_core::CoolErrorResponse>(
        codec,
        headers,
        &body_bytes,
    ) {
        Ok(parsed) => RpcErrorBody::from_cool_response(parsed),
        Err(_) => {
            // Handler emitted a non-2xx with a body that isn't the
            // framework's REST error shape (unusual — would happen if a
            // handler escaped through `into_response()` directly). Build
            // a synthetic body from the status alone.
            let cool = synthesize_error_for_status(status);
            RpcErrorBody::from_cool(&cool)
        }
    };

    encode_rpc_value_response(codec, headers, status, rpc_body)
}

fn encode_rpc_value_response<C, T>(
    codec: &C,
    headers: &HeaderMap,
    status: axum::http::StatusCode,
    value: T,
) -> axum::response::Response
where
    C: HttpTransport,
    T: Serialize,
{
    // Re-use the existing transport encoder so content negotiation
    // happens via the same path as everything else.
    crate::encode_transport_result_with_status_for::<_, T>(
        codec,
        headers,
        &RPC_BINDING_CAPABILITIES,
        status,
        Ok(value),
    )
}

// -----------------------------------------------------------------------------
// Batch helpers
// -----------------------------------------------------------------------------

/// Convert an [`axum::Response`] returned by an inner dispatch arm into a
/// single batch response frame.
///
/// Success bodies (2xx) are decoded as `serde_json::Value` via the same
/// codec the request used and become `RpcResponseFrame::ok`. Error
/// bodies (4xx/5xx) — which have already been post-processed by
/// [`convert_handler_error_response`] inside `rpc_dispatch_inner` — are
/// decoded as [`RpcErrorBody`] and inlined into
/// `RpcResponseFrame::error` directly.
///
/// Wire limitation: success outputs must be representable as
/// `serde_json::Value`. For CRUD/procedure outputs this is fine; if a
/// future op returns CBOR-only types (e.g. raw byte strings without a
/// JSON representation) the frame becomes an `internal` error.
pub async fn response_to_frame<C>(
    id: u64,
    response: axum::response::Response,
    codec: &C,
    headers: &HeaderMap,
) -> RpcResponseFrame
where
    C: HttpTransport,
{
    let status = response.status();
    let body_bytes = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
        Ok(bytes) => bytes.to_vec(),
        Err(error) => {
            return RpcResponseFrame::err(
                id,
                &CoolError::Internal(format!("buffer batch frame body: {error}")),
            );
        }
    };

    if status.is_success() {
        match decode_rpc_body::<_, serde_json::Value>(codec, headers, &body_bytes) {
            Ok(value) => RpcResponseFrame::ok(id, value),
            Err(error) => RpcResponseFrame::err(id, &error),
        }
    } else {
        // Body is already RpcErrorBody-shaped — `rpc_dispatch_inner`
        // post-processes handler errors before they reach us.
        match decode_rpc_body::<_, RpcErrorBody>(codec, headers, &body_bytes) {
            Ok(body) => RpcResponseFrame {
                id,
                output: None,
                error: Some(body),
            },
            Err(_) => {
                // Defensive: a handler/dispatcher returned a non-2xx
                // body that isn't RpcErrorBody-shaped. Synthesize one
                // from the status alone rather than corrupting the
                // batch envelope.
                let synthetic = synthesize_error_for_status(status);
                RpcResponseFrame::err(id, &synthetic)
            }
        }
    }
}

fn synthesize_error_for_status(status: axum::http::StatusCode) -> CoolError {
    let code = status.as_u16();
    let suffix = format!("upstream returned {code}");
    match code {
        400 => CoolError::BadRequest(suffix),
        401 => CoolError::Unauthorized(suffix),
        403 => CoolError::Forbidden(suffix),
        404 => CoolError::NotFound(suffix),
        406 => CoolError::NotAcceptable(suffix),
        409 => CoolError::Conflict(suffix),
        412 => CoolError::PreconditionFailed(suffix),
        415 => CoolError::UnsupportedMediaType(suffix),
        422 => CoolError::Validation(suffix),
        _ => CoolError::Internal(suffix),
    }
}

// -----------------------------------------------------------------------------
// Codec helpers used by the macro-emitted dispatcher
// -----------------------------------------------------------------------------

const DEFAULT_CONTENT_TYPE: &str = "application/cbor";

/// Decode an RPC unary request body into `T`, picking the codec based on
/// the request's `Content-Type` header. Missing header → CBOR (the
/// default for the REST binding too).
///
/// Used by the macro-generated RPC dispatcher; safe to use directly.
//
// TODO: this is nearly identical to `decode_transport_request_for` but
// differs in the missing-Content-Type fallback — this helper defaults to
// CBOR, while `decode_transport_request_for` errors with
// `UnsupportedMediaType`. Reconciling the two would change RPC behavior,
// so the bodies are kept distinct for now.
pub fn decode_rpc_body<C, T>(codec: &C, headers: &HeaderMap, body: &[u8]) -> Result<T, CoolError>
where
    C: HttpTransport,
    T: for<'de> Deserialize<'de>,
{
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or(DEFAULT_CONTENT_TYPE);
    codec.decode_request(content_type, body)
}

/// Encode an arbitrary serializable value back to bytes using the same
/// codec as the request. Used by the macro-generated `update` dispatch
/// arm to re-encode the typed patch before handing it to the existing
/// update handler as `Bytes`.
///
/// Async because the codec's `encode_response` returns an `axum::Response`
/// whose body has to be buffered out — in practice the codec always
/// produces an in-memory `Full<Bytes>` body, so this completes in one
/// poll, but we don't depend on that.
pub async fn encode_rpc_value<C, T>(
    codec: &C,
    headers: &HeaderMap,
    value: &T,
) -> Result<Vec<u8>, CoolError>
where
    C: HttpTransport,
    T: Serialize + ?Sized,
{
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or(DEFAULT_CONTENT_TYPE);
    let response = codec.encode_response(content_type, axum::http::StatusCode::OK, value)?;
    let (_parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|error| {
            CoolError::Internal(format!("failed to buffer encoded RPC body: {error}"))
        })?;
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cool_error_code_to_rpc_code_covers_every_cool_error_variant() {
        // Mirror image of `rpc_code_maps_each_cool_error_variant` — for
        // every CoolError variant, encoding it as CoolErrorResponse and
        // then translating its `code` must land on the same gRPC-style
        // string as the direct `rpc_code` path.
        for variant in [
            CoolError::BadRequest("x".into()),
            CoolError::NotAcceptable("x".into()),
            CoolError::Unauthorized("x".into()),
            CoolError::UnsupportedMediaType("x".into()),
            CoolError::Forbidden("x".into()),
            CoolError::NotFound("x".into()),
            CoolError::Conflict("x".into()),
            CoolError::Validation("x".into()),
            CoolError::PreconditionFailed("x".into()),
            CoolError::Codec("x".into()),
            CoolError::Database("x".into()),
            CoolError::Internal("x".into()),
        ] {
            let cool_code = variant.code();
            let direct = rpc_code(&variant);
            let translated = cool_error_code_to_rpc_code(cool_code);
            assert_eq!(
                direct, translated,
                "rpc_code({:?}) = {:?} but cool_error_code_to_rpc_code({:?}) = {:?}",
                variant, direct, cool_code, translated,
            );
        }
    }

    #[test]
    fn cool_error_code_to_rpc_code_unknown_input_falls_to_internal() {
        // A server that adds a new CoolError variant we don't know about
        // shouldn't leak a SCREAMING string to the wire — degrade to
        // "internal" rather than passing through.
        assert_eq!(cool_error_code_to_rpc_code("SOMETHING_NEW"), "internal");
        assert_eq!(cool_error_code_to_rpc_code(""), "internal");
    }

    #[test]
    fn error_body_from_cool_response_translates_code_and_preserves_message() {
        let response = cratestack_core::CoolErrorResponse {
            code: "NOT_FOUND".to_owned(),
            message: "widget 42".to_owned(),
            details: None,
        };
        let body = RpcErrorBody::from_cool_response(response);
        assert_eq!(body.code, "not_found");
        assert_eq!(body.message, "widget 42");
        assert!(body.details.is_none());
    }

    #[test]
    fn rpc_code_maps_each_cool_error_variant() {
        assert_eq!(rpc_code(&CoolError::BadRequest("x".into())), "invalid_argument");
        assert_eq!(rpc_code(&CoolError::NotAcceptable("x".into())), "invalid_argument");
        assert_eq!(rpc_code(&CoolError::Unauthorized("x".into())), "unauthenticated");
        assert_eq!(
            rpc_code(&CoolError::UnsupportedMediaType("x".into())),
            "invalid_argument",
        );
        assert_eq!(rpc_code(&CoolError::Forbidden("x".into())), "permission_denied");
        assert_eq!(rpc_code(&CoolError::NotFound("x".into())), "not_found");
        assert_eq!(rpc_code(&CoolError::Conflict("x".into())), "conflict");
        assert_eq!(rpc_code(&CoolError::Validation("x".into())), "invalid_argument");
        assert_eq!(
            rpc_code(&CoolError::PreconditionFailed("x".into())),
            "failed_precondition",
        );
        assert_eq!(rpc_code(&CoolError::Codec("x".into())), "invalid_argument");
        assert_eq!(rpc_code(&CoolError::Database("x".into())), "internal");
        assert_eq!(rpc_code(&CoolError::Internal("x".into())), "internal");
    }

    #[test]
    fn error_body_uses_public_message_not_operator_detail() {
        // 5xx variants must return the canned public message, never the
        // operator-only detail string carried inside the variant.
        let body = RpcErrorBody::from_cool(&CoolError::Internal("db ip refused".into()));
        assert_eq!(body.code, "internal");
        assert_eq!(body.message, "internal error");
        assert!(
            !body.message.contains("db ip refused"),
            "internal error detail leaked to the wire: {}",
            body.message,
        );
    }

    #[test]
    fn error_body_uses_caller_supplied_message_for_4xx() {
        let body = RpcErrorBody::from_cool(&CoolError::NotFound("widget 42".into()));
        assert_eq!(body.code, "not_found");
        assert_eq!(body.message, "widget 42");
    }

    #[test]
    fn synthesize_list_query_returns_none_when_empty() {
        let input = RpcListInput::default();
        assert!(synthesize_list_query(&input).is_none());
    }

    #[test]
    fn synthesize_list_query_round_trips_through_parse_query_pairs() {
        let mut include_fields = std::collections::BTreeMap::new();
        include_fields.insert("author".to_owned(), vec!["id".to_owned(), "name".to_owned()]);

        let input = RpcListInput {
            limit: Some(20),
            offset: Some(40),
            fields: Some(vec!["id".to_owned(), "title".to_owned()]),
            include: Some(vec!["author".to_owned()]),
            include_fields,
            sort: Some("createdAt desc".to_owned()),
            where_expr: Some("published=true".to_owned()),
            or: None,
            filters: vec![RpcListPredicate {
                key: "authorId".to_owned(),
                value: "42".to_owned(),
            }],
        };

        let query = synthesize_list_query(&input).expect("input not empty, query should exist");
        let pairs = crate::parse_query_pairs(Some(&query)).expect("synthesized query parses");

        // Every input field re-appears in the parsed pairs with the right key.
        // The parser strips no information, so this is a faithful round-trip.
        let has = |k: &str, v: &str| {
            pairs
                .iter()
                .any(|(pk, pv)| pk == k && pv == v)
        };
        assert!(has("limit", "20"));
        assert!(has("offset", "40"));
        assert!(has("fields", "id,title"));
        assert!(has("include", "author"));
        assert!(has("includeFields[author]", "id,name"));
        assert!(has("sort", "createdAt desc"));
        assert!(has("where", "published=true"));
        assert!(has("authorId", "42"));
    }

    #[test]
    fn response_frame_ok_and_err_are_mutually_exclusive() {
        let ok = RpcResponseFrame::ok(1, serde_json::json!({"x": 1}));
        assert!(ok.output.is_some());
        assert!(ok.error.is_none());

        let err = RpcResponseFrame::err(2, &CoolError::NotFound("x".into()));
        assert!(err.output.is_none());
        assert!(err.error.is_some());
    }
}
