//! [`CallBinding`]: the owned, FFI-shaped inputs of one call's [`Binding`].
//!
//! The Flutter and wasm glues (cratestack#1026) receive a call's binding
//! across a bridge, where borrowed `Cow`s and `PathParams` cannot cross.
//! This type is what they map into, so neither re-implements the canonical
//! query or the AAD inputs: both go through [`CallBinding::request`] and
//! [`CallBinding::response`], which build the same [`Binding`] the Rust
//! client builds (`cratestack-client-rust`'s `request_sealed`).
//!
//! ```
//! use cratestack_cose::CallBinding;
//!
//! let call = CallBinding {
//!     audience: "payments".into(),
//!     method: "GET".into(),
//!     route: "/accounts/{id}".into(),
//!     path_params: vec!["acc_42".into()],
//!     // Spelled out of order: the binding carries the canonical form.
//!     query: Some("b=2&a=1".into()),
//!     contract_sha: [7; 32],
//!     idempotency_key: None,
//!     if_match: None,
//! };
//! let request = call.request();
//! assert_eq!(request.query.as_deref(), Some("a=1&b=2"));
//! assert_eq!(request.payload_media_type, "application/cbor");
//! assert!(request.response.is_none());
//!
//! let response = call.response(b"the sealed request bytes", 200);
//! assert_eq!(response.response.unwrap().status, 200);
//! ```

use std::borrow::Cow;

use cratestack_core::{
    Binding, BoundHeaders, ContractSelector, PathParams, ResponseBinding, canonical_query,
    request_digest,
};

/// The payload media type of every Dart and wasm call: the CBOR codec's.
const PAYLOAD_MEDIA_TYPE: &str = "application/cbor";

/// Everything one call binds, owned.
///
/// `query` is the raw query string as the client will send it; it is
/// canonicalised here, and an empty result binds as `None`, as it does in
/// the Rust client. `idempotency_key` and `if_match` are bound exactly as
/// the request header will carry them, so the caller must send those
/// headers byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallBinding {
    /// The configured name of the service addressed (never the `Host`).
    pub audience: String,
    /// HTTP method, e.g. `"POST"`.
    pub method: String,
    /// The RPC `op_id`, or the REST route template.
    pub route: String,
    /// REST path parameter values in template order; empty for RPC.
    pub path_params: Vec<String>,
    /// The request's query string, in any spelling.
    pub query: Option<String>,
    /// The op contract digest (`OP_CONTRACTS` of the generated client).
    pub contract_sha: [u8; 32],
    /// The request's `Idempotency-Key` header value, if it carries one.
    pub idempotency_key: Option<String>,
    /// The request's `If-Match` header value, if it carries one.
    pub if_match: Option<String>,
}

impl CallBinding {
    /// The request binding: what a request is sealed and opened under.
    pub fn request(&self) -> Binding<'_> {
        let query = Some(canonical_query(self.query.as_deref())).filter(|query| !query.is_empty());
        Binding {
            audience: Cow::Borrowed(&self.audience),
            method: Cow::Borrowed(&self.method),
            route: Cow::Borrowed(&self.route),
            path_params: PathParams::Owned(self.path_params.clone()),
            query: query.map(Cow::Owned),
            contract_sha: self.contract_sha,
            payload_media_type: Cow::Borrowed(PAYLOAD_MEDIA_TYPE),
            bound_headers: BoundHeaders {
                idempotency_key: self.idempotency_key.as_deref().map(Cow::Borrowed),
                if_match: self.if_match.as_deref().map(Cow::Borrowed),
            },
            response: None,
        }
    }

    /// The binding of the response to `sealed_request` (the exact bytes
    /// that travelled) with the HTTP `status` it came back with.
    pub fn response(&self, sealed_request: &[u8], status: u16) -> Binding<'_> {
        Binding {
            response: Some(ResponseBinding {
                request: request_digest(sealed_request),
                status,
            }),
            ..self.request()
        }
    }
}

/// The `Cratestack-Contract` header value for an op contract digest: the
/// first 8 bytes as 11 characters of unpadded base64url. The header is not
/// bound; it tells a server holding several accepted digests which one the
/// request was sealed under.
pub fn contract_header_value(contract_sha: &[u8; 32]) -> String {
    ContractSelector::of(contract_sha).to_header_value()
}

#[cfg(test)]
mod tests;
