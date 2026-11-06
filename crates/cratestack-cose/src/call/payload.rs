//! The payload-type half of a [`CallBinding`]: validation, the selector
//! headers, and the response's type (cratestack#1168, compact wire keys).
//!
//! The rules are `cratestack-client-rust`'s `check_payload_types` and
//! `PendingResponse::open`, so a Dart or wasm app seals exactly what the
//! Rust client does.

use std::borrow::Cow;

use cratestack_core::{
    Binding, CratestackError, DEFAULT_PAYLOAD_MEDIA_TYPE, PAYLOAD_ACCEPT_HEADER,
    PAYLOAD_TYPE_HEADER, ResponseBinding, is_sealable_payload_type, parse_payload_accept,
    parse_payload_type, request_digest,
};

use super::CallBinding;

impl CallBinding {
    /// The type the request payload is sealed under: the CBOR codec's
    /// unless [`payload_type`](Self::payload_type) says otherwise.
    pub fn request_payload_type(&self) -> &str {
        self.payload_type
            .as_deref()
            .unwrap_or(DEFAULT_PAYLOAD_MEDIA_TYPE)
    }

    /// The types the call reads its answer in, in order of preference.
    pub fn accepted_payload_types(&self) -> Vec<&str> {
        match &self.payload_accept {
            Some(types) => types.iter().map(String::as_str).collect(),
            None => vec![DEFAULT_PAYLOAD_MEDIA_TYPE],
        }
    }

    /// Check the payload type and the accept list: each must be sealable
    /// (`cratestack_core::is_sealable_payload_type`, so never an envelope,
    /// a `+cbor-seq` stream or a multipart body) and the list well formed.
    /// Fails with `Validation`, before anything is sealed.
    pub fn validate(&self) -> Result<(), CratestackError> {
        let payload_type = self.request_payload_type();
        if !is_sealable_payload_type(payload_type) {
            return Err(CratestackError::Validation(format!(
                "a COSE envelope cannot wrap a payload of type {payload_type:?}: a payload type \
                 is a lowercase type/subtype without parameters, and never an envelope, a \
                 stream or a multipart body"
            )));
        }
        let joined = self.accepted_payload_types().join(", ");
        let accept = parse_payload_accept(joined.as_bytes())
            .map_err(|error| CratestackError::Validation(error.to_string()))?;
        match accept.iter().find(|entry| !is_sealable_payload_type(entry)) {
            Some(entry) => Err(CratestackError::Validation(format!(
                "a COSE envelope cannot carry an answer of type {entry:?}"
            ))),
            None => Ok(()),
        }
    }

    /// The payload-type selector headers this call must send, as
    /// `(name, value)`: `Cratestack-Payload-Type` when the request type is
    /// not CBOR, `Cratestack-Payload-Accept` when the accept list is not
    /// just CBOR. Empty for a default call, which sends neither. They are
    /// not themselves signed; the binding makes a wrong one fail
    /// verification.
    pub fn selector_headers(&self) -> Vec<(&'static str, String)> {
        let mut headers = Vec::new();
        if self.request_payload_type() != DEFAULT_PAYLOAD_MEDIA_TYPE {
            headers.push((PAYLOAD_TYPE_HEADER, self.request_payload_type().to_owned()));
        }
        let accept = self.accepted_payload_types();
        if accept != [DEFAULT_PAYLOAD_MEDIA_TYPE] {
            headers.push((PAYLOAD_ACCEPT_HEADER, accept.join(", ")));
        }
        headers
    }

    /// The binding of the response to `sealed_request` with the HTTP
    /// `status`, sealed under the type the response's
    /// `Cratestack-Payload-Type` header named (`response_type`; `None`
    /// means CBOR). The type is the binding's `payload_media_type`.
    ///
    /// A type the call did not ask for (not in the accept list) is
    /// `Unauthorized`, the coarse refusal, before any body is opened. The
    /// header is not authenticated by itself: it names the string the
    /// binding is rebuilt with, and a wrong one fails verification.
    pub fn response_for(
        &self,
        sealed_request: &[u8],
        status: u16,
        response_type: Option<&str>,
    ) -> Result<Binding<'_>, CratestackError> {
        let named = match response_type {
            None => DEFAULT_PAYLOAD_MEDIA_TYPE,
            Some(text) => parse_payload_type(text.as_bytes()).map_err(|_| unexpected())?,
        };
        let accepted = self.accepted_payload_types();
        let Some(&payload) = accepted.iter().find(|entry| **entry == named) else {
            return Err(unexpected());
        };
        let mut binding = self.request();
        binding.payload_media_type = Cow::Owned(payload.to_owned());
        binding.response = Some(ResponseBinding {
            request: request_digest(sealed_request),
            status,
        });
        Ok(binding)
    }
}

/// A response type the call did not ask for: the coarse refusal, with no
/// detail (the received header is untrusted).
fn unexpected() -> CratestackError {
    CratestackError::Unauthorized(crate::UNAUTHENTICATED.to_owned())
}
