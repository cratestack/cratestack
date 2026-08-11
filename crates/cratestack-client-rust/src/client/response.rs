use reqwest::StatusCode;

use crate::runtime::wire::RuntimeHeader;

/// A decoded typed body paired with the status and headers it arrived
/// with (issue #493).
///
/// `CratestackClient::{get,post,patch,delete}` decode straight to
/// `Output`, discarding everything else about the response — which
/// makes an `@version` model's optimistic-locking contract
/// unreachable through the typed surface: the required round trip is
/// `GET` → read `ETag` → `PATCH` with `If-Match`, and the middle step
/// needs a header that plain `get` throws away. `TypedResponse` exists
/// so a caller can opt into the metadata via the `*_with_response`
/// methods without touching the original signatures at all — every
/// existing call site keeps compiling unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedResponse<Output> {
    pub value: Output,
    pub status: StatusCode,
    pub headers: Vec<RuntimeHeader>,
}

impl<Output> TypedResponse<Output> {
    /// Case-insensitive header lookup — HTTP header names are
    /// case-insensitive per RFC 7230 §3.2, but `RuntimeHeader` stores
    /// them as plain strings (the wire type is also used across the
    /// FFI boundary, where a `HeaderMap` isn't available), so a caller
    /// doing `.header("ETag")` against a server that sent `etag` must
    /// not silently miss it.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_lookup_is_case_insensitive() {
        let response = TypedResponse {
            value: (),
            status: StatusCode::OK,
            headers: vec![RuntimeHeader {
                name: "ETag".to_owned(),
                value: "\"0\"".to_owned(),
            }],
        };

        assert_eq!(response.header("etag"), Some("\"0\""));
        assert_eq!(response.header("ETAG"), Some("\"0\""));
        assert_eq!(response.header("If-Match"), None);
    }
}
