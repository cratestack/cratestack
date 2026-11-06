//! Reading the compact-related request headers.

use axum::http::{HeaderMap, HeaderValue, header};
use cratestack_core::compact::{
    COMPACT_CBOR, COMPACT_CBOR_SEQ, COMPACT_JSON, compact_media_type_for, plain_media_type_for,
};
use cratestack_core::{CONTRACT_HEADER, ContractSelector};

/// The media type of `value` without parameters, lowercased.
fn base(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn refused(entry: &str) -> bool {
    entry
        .split(';')
        .skip(1)
        .filter_map(|param| param.trim().strip_prefix("q="))
        .any(|q| q.trim().parse::<f32>().is_ok_and(|q| q == 0.0))
}

fn accept_entries(headers: &HeaderMap) -> impl Iterator<Item = &str> {
    headers
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
}

/// The compact types `Accept` lists with a non-zero weight.
pub(super) fn accepted_compact(headers: &HeaderMap) -> Vec<&'static str> {
    let mut found = Vec::new();
    for entry in accept_entries(headers).filter(|entry| !refused(entry)) {
        let compact = match base(entry).as_str() {
            COMPACT_CBOR => COMPACT_CBOR,
            COMPACT_JSON => COMPACT_JSON,
            COMPACT_CBOR_SEQ => COMPACT_CBOR_SEQ,
            _ => continue,
        };
        if !found.contains(&compact) {
            found.push(compact);
        }
    }
    found
}

/// Replace every compact type in `Accept` by its plain twin (keeping its
/// parameters), dropping repeats, so the handler negotiates as usual.
pub(super) fn rewrite_accept(headers: &mut HeaderMap) {
    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for entry in accept_entries(headers) {
        let kind = base(entry);
        let (kind, entry) = match plain_media_type_for(&kind) {
            Some(plain) => {
                let params = entry.split_once(';').map(|(_, rest)| rest).unwrap_or("");
                let entry = if params.is_empty() {
                    plain.to_owned()
                } else {
                    format!("{plain};{params}")
                };
                (plain.to_owned(), entry)
            }
            None => (kind, entry.to_owned()),
        };
        if !seen.contains(&kind) {
            seen.push(kind);
            out.push(entry);
        }
    }
    headers.remove(header::ACCEPT);
    if let Ok(value) = HeaderValue::from_str(&out.join(", ")) {
        headers.insert(header::ACCEPT, value);
    }
}

/// The request body's compact type, if its `Content-Type` is one.
pub(super) fn request_compact(headers: &HeaderMap) -> Option<&'static str> {
    let value = headers.get(header::CONTENT_TYPE)?.to_str().ok()?;
    let base = base(value);
    let plain = plain_media_type_for(&base)?;
    compact_media_type_for(plain)
}

/// The single `Cratestack-Contract` selector, `None` when absent, repeated
/// or malformed (an unsigned call then simply stays named).
pub(super) fn selector(headers: &HeaderMap) -> Option<ContractSelector> {
    let mut values = headers.get_all(CONTRACT_HEADER).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    ContractSelector::from_header_value(value.as_bytes()).ok()
}

/// Whether `compact` is a unary (non-sequence) type a request body may use.
pub(super) fn is_unary(compact: &str) -> bool {
    compact == COMPACT_CBOR || compact == COMPACT_JSON
}

/// The plain base type of a response `Content-Type`.
pub(super) fn response_base(headers: &HeaderMap) -> Option<String> {
    Some(base(headers.get(header::CONTENT_TYPE)?.to_str().ok()?))
}
