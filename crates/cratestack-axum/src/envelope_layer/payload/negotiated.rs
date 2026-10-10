//! What a request negotiated (cratestack#1168).

use cratestack_core::DEFAULT_PAYLOAD_MEDIA_TYPE;

pub(super) const JSON: &str = "application/json";

/// What a request negotiated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::envelope_layer) struct Negotiated {
    /// The type of the request payload: the one its binding names.
    pub(in crate::envelope_layer) request: String,
    /// The types the response may be sealed under, in the client's order of
    /// preference. Never empty, and always holds one the transport's error
    /// codec can write ([`Self::error_type`]).
    pub(in crate::envelope_layer) response: Vec<String>,
    /// Whether the request's type was left for [`check_opened`](super::check_opened): a method
    /// that carries no payload of its own.
    pub(in crate::envelope_layer) deferred: bool,
}

impl Negotiated {
    /// The type the layer's own errors and re-encoded handler errors are
    /// sealed in: the client's first choice among the two the middleware
    /// error codec writes (CBOR, JSON).
    pub(in crate::envelope_layer) fn error_type(&self) -> &str {
        self.response
            .iter()
            .map(String::as_str)
            .find(|candidate| matches!(*candidate, DEFAULT_PAYLOAD_MEDIA_TYPE | JSON))
            .unwrap_or(DEFAULT_PAYLOAD_MEDIA_TYPE)
    }

    /// The negotiated response type `media_type` names (case-insensitively,
    /// as a handler's `Content-Type` is not held to the lowercase grammar),
    /// spelled the way it was negotiated.
    pub(in crate::envelope_layer) fn response_type(&self, media_type: &str) -> Option<&str> {
        self.response
            .iter()
            .map(String::as_str)
            .find(|candidate| candidate.eq_ignore_ascii_case(media_type))
    }

    /// The `Accept` the router sees: the negotiated types, the client's order.
    pub(in crate::envelope_layer) fn accept(&self) -> String {
        self.response.join(", ")
    }
}
