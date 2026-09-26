//! `/rpc/batch` (decisions D11 and B1): one unary message bound as
//! `batch`, run under the strictest mode of `batch` and of every frame's
//! op, so a batch cannot carry an op its own policy would refuse unsigned.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::{CratestackCodec, CratestackError};
use http::header::CONTENT_TYPE;
use http::request::Parts;
use http::{HeaderMap, Method};
use serde::Deserialize;

use super::dispatch::by_mode;
use super::layer::Config;
use super::mode::EnvelopeMode;
use super::policy_request::PolicyRequest;
use super::resolver::ResolvedRoute;
use super::service::{Inner, call};
use super::{PAYLOAD_MEDIA_TYPE, refusal, request};

/// A frame's op, and nothing else: every other field is skipped, whatever
/// it holds, so any body the batch handler can decode (with the same codec)
/// decodes here too.
#[derive(Deserialize)]
struct FrameOp {
    op: String,
}

/// The frames' op ids, decoded as the generated handler decodes the body:
/// by the first `Content-Type` (absent, or not text, means CBOR, as in
/// `decode_rpc_body`), with the first-party codecs. `None` when it cannot:
/// another type (a custom codec), or a body that is not a frame array.
///
/// Matched on the **base** type, with parameters ignored (security finding
/// SF-1 of the second review): the handler's codecs accept
/// `application/cbor; charset=binary`, so an exact match here left the
/// frames unread while the handler ran them, and under an
/// `unresolved_mode` of `Off` a batch carrying a `Required` op went
/// through plain. Case is ignored too, which the first-party handler does
/// not do: it only makes the layer read a body the handler may refuse,
/// the fail-closed side, and covers a custom codec that is lenient.
fn frame_ops(content_type: Option<&str>, body: &[u8]) -> Option<Vec<String>> {
    let content_type = content_type.unwrap_or(PAYLOAD_MEDIA_TYPE);
    let base = content_type.split(';').next().unwrap_or(content_type).trim();
    let frames: Vec<FrameOp> = if base.eq_ignore_ascii_case("application/cbor") {
        CborCodec.decode(body).ok()?
    } else if base.eq_ignore_ascii_case("application/json") {
        JsonCodec.decode(body).ok()?
    } else {
        return None;
    };
    Some(frames.into_iter().map(|frame| frame.op).collect())
}

/// The strictest of `batch_mode` and every frame op's mode; with no frames
/// to read, of `batch_mode` and the policy's `unresolved_mode`.
fn verdict(
    config: &Config,
    method: &Method,
    batch: EnvelopeMode,
    ops: Option<&[String]>,
) -> EnvelopeMode {
    let Some(ops) = ops else {
        return batch.strictest(config.policy.unresolved_mode());
    };
    ops.iter().fold(batch, |mode, op| {
        mode.strictest(
            config
                .policy
                .mode(&PolicyRequest::new(method, op).batch_frame()),
        )
    })
}

fn batch_mode(config: &Config, method: &Method) -> EnvelopeMode {
    config.policy.mode(&PolicyRequest::new(method, "batch"))
}

fn content_type(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
}

pub(super) async fn handle<S: Inner>(
    config: Arc<Config>,
    inner: S,
    parts: Parts,
    body: Body,
    route: ResolvedRoute,
    enveloped: bool,
) -> Response {
    let batch = batch_mode(&config, &parts.method);
    if enveloped {
        // The frames are sealed: until verified they are unreadable, so the
        // mode before opening assumes the worst of them. The frames' own
        // modes are checked once opened (`signed_verdict`).
        let before = batch.strictest(config.policy.unresolved_mode());
        return by_mode(config, inner, parts, body, route, true, before).await;
    }
    if batch == EnvelopeMode::Required {
        return refusal::unauthenticated(&parts.headers, parts.uri.path());
    }
    let raw = match request::buffer(body, config.max_body_bytes).await {
        Ok(raw) => raw,
        Err(error) => return refusal::unbuffered(&parts.headers, parts.uri.path(), error),
    };
    let ops = frame_ops(content_type(&parts.headers), &raw);
    let mode = verdict(&config, &parts.method, batch, ops.as_deref());
    if ops.is_none() && mode != EnvelopeMode::Off {
        // Refused, not forwarded: which ops it would run is unknown.
        if mode == EnvelopeMode::Required {
            return refusal::unauthenticated(&parts.headers, parts.uri.path());
        }
        let error = CratestackError::BadRequest("the batch frames could not be read".to_owned());
        return refusal::bad_request(&parts.headers, parts.uri.path(), error);
    }
    match mode {
        EnvelopeMode::Off => call(inner, Request::from_parts(parts, Body::from(raw))).await,
        mode => by_mode(config, inner, parts, Body::from(raw), route, false, mode).await,
    }
}

/// For an opened `/rpc/batch` payload (CBOR): the mode its frames require,
/// or the error to seal (`400`) when they cannot be read. The call is
/// refused with the `415` if every answer is `Off`, as a signed unary call
/// to an `Off` op is.
pub(super) fn signed_verdict(
    config: &Config,
    method: &Method,
    payload: &[u8],
) -> Result<EnvelopeMode, CratestackError> {
    let ops = frame_ops(Some(PAYLOAD_MEDIA_TYPE), payload).ok_or_else(|| {
        CratestackError::BadRequest("the batch frames could not be read".to_owned())
    })?;
    Ok(verdict(
        config,
        method,
        batch_mode(config, method),
        Some(&ops),
    ))
}
