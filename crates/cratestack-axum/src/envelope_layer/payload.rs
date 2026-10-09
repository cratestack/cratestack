//! Which payload types a request is sealed under and may be answered in
//! (cratestack#1168).
//!
//! Read from the two unbound selector headers (`cratestack_core::PAYLOAD_TYPE_HEADER`,
//! `PAYLOAD_ACCEPT_HEADER`) **before** any key is looked up, any signature
//! checked or any nonce spent, so a refusal costs nothing and is unsigned,
//! like the `426` contract refusal. The headers select; they never widen:
//! the AAD carries the whole type, so a lie fails verification.
//!
//! What an op allows is the layer's opt-in set
//! ([`EnvelopeLayerBuilder::payload_media_types`], CBOR alone by default)
//! intersected with the route's declared set
//! ([`ResolvedRoute::with_payload_types`], which the built-in resolvers
//! fill from the schema's descriptors). `/rpc/batch` is CBOR whatever either
//! says: its frames are CBOR.
//!
//! [`EnvelopeLayerBuilder::payload_media_types`]: super::EnvelopeLayerBuilder::payload_media_types

use cratestack_core::{
    CratestackError, DEFAULT_PAYLOAD_MEDIA_TYPE, PAYLOAD_ACCEPT_HEADER, PAYLOAD_TYPE_HEADER,
    parse_payload_accept, parse_payload_type,
};
use http::HeaderMap;

use super::layer::Config;
use super::resolver::ResolvedRoute;

const JSON: &str = "application/json";

/// What a request negotiated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Negotiated {
    /// The type of the request payload: the one its binding names.
    pub(super) request: String,
    /// The types the response may be sealed under, in the client's order of
    /// preference. Never empty, and always holds one the transport's error
    /// codec can write ([`Self::error_type`]).
    pub(super) response: Vec<String>,
}

impl Negotiated {
    /// The type the layer's own errors and re-encoded handler errors are
    /// sealed in: the client's first choice among the two the middleware
    /// error codec writes (CBOR, JSON).
    pub(super) fn error_type(&self) -> &str {
        self.response
            .iter()
            .map(String::as_str)
            .find(|candidate| matches!(*candidate, DEFAULT_PAYLOAD_MEDIA_TYPE | JSON))
            .unwrap_or(DEFAULT_PAYLOAD_MEDIA_TYPE)
    }

    /// The negotiated response type `media_type` names (case-insensitively,
    /// as a handler's `Content-Type` is not held to the lowercase grammar),
    /// spelled the way it was negotiated.
    pub(super) fn response_type(&self, media_type: &str) -> Option<&str> {
        self.response
            .iter()
            .map(String::as_str)
            .find(|candidate| candidate.eq_ignore_ascii_case(media_type))
    }

    /// The `Accept` the router sees: the negotiated types, the client's order.
    pub(super) fn accept(&self) -> String {
        self.response.join(", ")
    }
}

/// Why a request's payload types were refused; all answered unsigned.
#[derive(Debug)]
pub(super) enum Refusal {
    /// A selector header sent twice or malformed: `400`.
    Malformed(CratestackError),
    /// The request's type is not allowed for the op: `415`.
    RequestType,
    /// None of the types the client reads may be sealed for the op: `406`.
    NotAcceptable,
}

/// The one value of `name`, `None` when absent. Two headers are refused:
/// which one the client sealed under is not ours to guess.
fn one<'h>(headers: &'h HeaderMap, name: &str) -> Result<Option<&'h [u8]>, CratestackError> {
    let mut values = headers.get_all(name).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(CratestackError::BadRequest(format!(
            "{name} must be sent at most once"
        )));
    }
    Ok(Some(value.as_bytes()))
}

/// Whether the layer, the route and (for `/rpc/batch`) the batch frames all
/// allow `media_type` on this side.
fn permits(layer: &[String], route: Option<&[&str]>, batch: bool, media_type: &str) -> bool {
    if batch && media_type != DEFAULT_PAYLOAD_MEDIA_TYPE {
        return false;
    }
    layer.iter().any(|allowed| allowed == media_type)
        && route.is_none_or(|declared| declared.contains(&media_type))
}

/// Negotiate `headers` for `route`. `signed` is false for an unsigned,
/// nonce-bound request under `Optional`: its payload is not sealed, so only
/// the response is negotiated.
pub(super) fn negotiate(
    config: &Config,
    route: &ResolvedRoute,
    headers: &HeaderMap,
    signed: bool,
) -> Result<Negotiated, Refusal> {
    let batch = route.is_batch();
    let declared = route.payload_types();
    let request = match signed {
        false => DEFAULT_PAYLOAD_MEDIA_TYPE,
        true => match one(headers, PAYLOAD_TYPE_HEADER).map_err(Refusal::Malformed)? {
            None => DEFAULT_PAYLOAD_MEDIA_TYPE,
            Some(value) => parse_payload_type(value).map_err(Refusal::Malformed)?,
        },
    };
    let accept = match one(headers, PAYLOAD_ACCEPT_HEADER).map_err(Refusal::Malformed)? {
        None => vec![DEFAULT_PAYLOAD_MEDIA_TYPE],
        Some(value) => parse_payload_accept(value).map_err(Refusal::Malformed)?,
    };
    if signed
        && !permits(
            &config.payload_request,
            declared.map(|types| types.request),
            batch,
            request,
        )
    {
        return Err(Refusal::RequestType);
    }
    let response: Vec<String> = accept
        .into_iter()
        .filter(|candidate| {
            permits(
                &config.payload_response,
                declared.map(|types| types.response),
                batch,
                candidate,
            )
        })
        .map(str::to_owned)
        .collect();
    let negotiated = Negotiated {
        request: request.to_owned(),
        response,
    };
    // Nothing to answer in, or nothing an error could be sealed in either.
    let writable = negotiated
        .response
        .iter()
        .any(|candidate| matches!(candidate.as_str(), DEFAULT_PAYLOAD_MEDIA_TYPE | JSON));
    match writable {
        true => Ok(negotiated),
        false => Err(Refusal::NotAcceptable),
    }
}
