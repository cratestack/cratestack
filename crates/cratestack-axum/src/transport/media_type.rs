use std::cmp::Reverse;

use axum::http::{HeaderMap, header};
use cratestack_core::{CoolError, RouteTransportCapabilities};

use super::http_transport::HttpTransport;

pub(crate) fn validate_transport_accept_header(
    headers: &HeaderMap,
    supported: &[&'static str],
) -> Result<(), CoolError> {
    let Some(accept) = headers.get(header::ACCEPT) else {
        return Ok(());
    };
    let accept = accept
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Accept header: {error}")))?;

    if supported
        .iter()
        .any(|content_type| accepts_content_type(accept, content_type))
    {
        Ok(())
    } else {
        Err(CoolError::NotAcceptable(format!(
            "router only serves {} responses",
            supported.join(", "),
        )))
    }
}

pub(crate) fn validate_transport_content_type_header(
    headers: &HeaderMap,
    supported: &[&'static str],
) -> Result<(), CoolError> {
    request_content_type(headers, supported).map(|_| ())
}

pub(crate) fn request_content_type(
    headers: &HeaderMap,
    supported: &[&'static str],
) -> Result<&'static str, CoolError> {
    let Some(content_type) = headers.get(header::CONTENT_TYPE) else {
        return Err(CoolError::UnsupportedMediaType(format!(
            "expected Content-Type one of {}",
            supported.join(", "),
        )));
    };
    let content_type = content_type
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Content-Type header: {error}")))?;

    supported
        .iter()
        .copied()
        .find(|expected| media_type_matches(content_type, expected))
        .ok_or_else(|| {
            CoolError::UnsupportedMediaType(format!(
                "expected Content-Type one of {}, got {}",
                supported.join(", "),
                content_type,
            ))
        })
}

/// Negotiates the response `Content-Type` for one route against the
/// concrete `transport`'s real encoders, not just `capabilities`'s
/// compile-time `response_types` list (cratestack#489): a router built
/// with a single `JsonCodec` still emits a `response_types` list naming
/// both `application/cbor` and `application/json`, so picking straight
/// from that list can select a type the router has no encoder for at
/// all, producing a 406 the caller never sees coming (`main`'s bug).
///
/// `capabilities.response_types` empty is a sentinel meaning "no
/// capabilities declared" — only the capability-free
/// `encode_transport_result`/`encode_transport_sequence_result` (and
/// `_with_status`) convenience wrappers pass that, and nothing in this
/// workspace calls them — so `default_response_type` is trusted as-is
/// there, same as before this fix, rather than inventing a policy for an
/// unconfigured caller.
pub(crate) fn select_transport_response_content_type<T>(
    transport: &T,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
) -> Result<&'static str, CoolError>
where
    T: HttpTransport,
{
    if capabilities.response_types.is_empty() {
        return Ok(capabilities.default_response_type);
    }
    let encodable: Vec<&'static str> = capabilities
        .response_types
        .iter()
        .copied()
        .filter(|content_type| transport.can_encode(content_type))
        .collect();
    select_response_content_type(headers, &encodable, capabilities.default_response_type)
}

/// Picks the response `Content-Type`, from `encodable` — the subset of a
/// route's advertised `response_types` the router's concrete transport can
/// actually encode (see [`select_transport_response_content_type`]) —
/// that best matches the client's `Accept` header. `encodable` is
/// pre-filtered by the caller, so the `NotAcceptable` this returns always
/// names something the router can genuinely produce.
///
/// Implements the RFC 9110 §12.5.1 negotiation contract properly: the
/// `Accept` header's ordering and `q=` weights drive the choice, not the
/// server's `encodable` list order. Previously this walked `encodable` and
/// returned the first entry the client merely *tolerated* — a client
/// sending `Accept: application/cbor-seq, application/cbor` to prefer
/// streaming and degrade gracefully to buffered cbor always got buffered
/// cbor back, because `encodable` (server order) put plain cbor first.
/// That silently broke `rpc-streaming-client-rust`, whose streaming
/// decoder has no buffered-cbor fallback (see `crates/cratestack-client-rust/
/// src/streaming.rs`) and dies on the first frame.
///
/// Each `Accept` entry is scored by `(q, specificity)`: an exact media-type
/// match outranks a `type/*` wildcard, which outranks `*/*`. Ties (equal
/// score) fall back first to whichever `Accept` entry appears earlier in
/// the header (the client's own tie-break signal), then to `encodable`'s
/// order (the server's stated preference) — so a client that sends no
/// `Accept` at all, or one with no q-values/wildcards, sees identical
/// behavior to before this fix.
pub(crate) fn select_response_content_type(
    headers: &HeaderMap,
    encodable: &[&'static str],
    default: &'static str,
) -> Result<&'static str, CoolError> {
    let Some(accept) = headers.get(header::ACCEPT) else {
        if encodable.contains(&default) {
            return Ok(default);
        }
        return encodable
            .first()
            .copied()
            .ok_or_else(|| not_acceptable_response(encodable));
    };
    let accept = accept
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Accept header: {error}")))?;

    let entries = parse_accept_header(accept);

    encodable
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(encodable_index, content_type)| {
            best_match_rank(&entries, content_type)
                .map(|rank| ((rank, Reverse(encodable_index)), content_type))
        })
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, content_type)| content_type)
        .ok_or_else(|| not_acceptable_response(encodable))
}

/// One parsed `Accept` header entry: a media type, its `q=` weight
/// (defaulting to `1.0`), and its zero-based position in the header —
/// position is kept as an explicit tie-breaker for entries with equal
/// weight, since a client listing several equally-weighted types is
/// still expressing an order.
struct AcceptEntry<'a> {
    media_type: &'a str,
    q_millis: u32,
    position: usize,
}

/// Score for how well one `Accept` entry matches a candidate content
/// type: compared lexicographically, so `q` dominates, specificity
/// breaks `q` ties, and earlier position in the `Accept` header breaks
/// specificity ties. `q=0` entries (explicitly rejected by the client,
/// RFC 9110 §12.5.1) never produce a rank at all — see [`best_match_rank`].
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
struct MatchRank {
    q_millis: u32,
    specificity: u8,
    neg_position: Reverse<usize>,
}

/// Parses an `Accept` header into weighted, ordered entries. Malformed
/// `q=` values fall back to `1.0` rather than rejecting the whole header
/// — a client's minor formatting slip shouldn't turn into a spurious 400
/// when the intent (list these types, most first) is still clear.
fn parse_accept_header(accept: &str) -> Vec<AcceptEntry<'_>> {
    accept
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .enumerate()
        .filter_map(|(position, entry)| {
            let mut parts = entry.split(';').map(str::trim);
            let media_type = parts.next()?;
            if media_type.is_empty() {
                return None;
            }
            let q_millis = parts
                .find_map(|param| {
                    param
                        .strip_prefix("q=")
                        .or_else(|| param.strip_prefix("Q="))
                })
                .and_then(|value| value.trim().parse::<f32>().ok())
                .map(|q| (q.clamp(0.0, 1.0) * 1000.0).round() as u32)
                .unwrap_or(1000);
            Some(AcceptEntry {
                media_type,
                q_millis,
                position,
            })
        })
        .collect()
}

/// Best (highest) [`MatchRank`] any parsed `Accept` entry gives
/// `content_type`, or `None` if nothing in `accept` matches it at all
/// (including everything matching only via a `q=0` entry, which RFC 9110
/// treats as an explicit rejection, not merely a low preference).
fn best_match_rank(accept: &[AcceptEntry<'_>], content_type: &'static str) -> Option<MatchRank> {
    accept
        .iter()
        .filter(|entry| entry.q_millis > 0)
        .filter_map(|entry| {
            let specificity = if entry.media_type == content_type {
                2
            } else if entry.media_type == wildcard_media_type(content_type) {
                1
            } else if entry.media_type == "*/*" {
                0
            } else {
                return None;
            };
            Some(MatchRank {
                q_millis: entry.q_millis,
                specificity,
                neg_position: Reverse(entry.position),
            })
        })
        .max()
}

fn not_acceptable_response(encodable: &[&'static str]) -> CoolError {
    if encodable.is_empty() {
        CoolError::NotAcceptable("router has no response encoder configured".to_owned())
    } else {
        CoolError::NotAcceptable(format!(
            "router only serves {} responses",
            encodable.join(", "),
        ))
    }
}

pub(crate) fn accepts_content_type(accept: &str, expected: &str) -> bool {
    accept.split(',').map(str::trim).any(|value| {
        if value == "*/*" {
            return true;
        }
        let media_type = strip_media_type_params(value);
        media_type == expected
            || media_type == wildcard_media_type(expected)
            || media_type == "application/*"
    })
}

pub(crate) fn media_type_matches(candidate: &str, expected: &str) -> bool {
    strip_media_type_params(candidate) == expected
}

pub(crate) fn strip_media_type_params(value: &str) -> &str {
    value.split(';').next().unwrap_or(value).trim()
}

pub(crate) fn wildcard_media_type(content_type: &str) -> &str {
    content_type
        .split_once('/')
        .map(|(prefix, _)| {
            if prefix == "application" {
                "application/*"
            } else {
                "*/*"
            }
        })
        .unwrap_or("*/*")
}

#[cfg(test)]
mod tests;
