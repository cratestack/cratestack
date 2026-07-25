//! `tonic::metadata::MetadataMap` <-> `http::HeaderMap` conversion.
//!
//! gRPC metadata *is* HTTP/2 headers on the wire — `tonic::metadata::MetadataMap`
//! is a thin wrapper around `http::HeaderMap` — so the existing header-driven
//! `cratestack_core::AuthProvider` ports to gRPC unchanged: build a
//! `cratestack_core::RequestContext` whose `headers` field is the
//! `http::HeaderMap` this module converts a gRPC request's metadata into,
//! and call the exact same `AuthProvider::authenticate` a REST/RPC schema
//! already calls.

/// Converts a tonic request's incoming metadata into the `http::HeaderMap`
/// `cratestack_core::RequestContext::headers` expects. Takes `&MetadataMap`
/// (tonic hands out `&MetadataMap` from `Request::metadata()`) and clones —
/// `MetadataMap`'s inner representation is an `http::HeaderMap` already, so
/// this is a cheap, allocation-free-per-entry clone, not a re-parse.
pub fn metadata_to_headers(metadata: &tonic::metadata::MetadataMap) -> http::HeaderMap {
    metadata.clone().into_headers()
}

/// The inverse — building outgoing (or test) metadata from a plain
/// `http::HeaderMap`.
pub fn headers_to_metadata(headers: http::HeaderMap) -> tonic::metadata::MetadataMap {
    tonic::metadata::MetadataMap::from_headers(headers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderMap, HeaderValue};

    #[test]
    fn round_trips_through_headers_and_back() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer abc"));
        headers.insert("x-request-id", HeaderValue::from_static("req-1"));

        let metadata = headers_to_metadata(headers.clone());
        let back = metadata_to_headers(&metadata);

        assert_eq!(back.get("authorization"), headers.get("authorization"));
        assert_eq!(back.get("x-request-id"), headers.get("x-request-id"));
        assert_eq!(back.len(), headers.len());
    }

    #[test]
    fn empty_metadata_yields_empty_headers() {
        let metadata = tonic::metadata::MetadataMap::new();
        assert!(metadata_to_headers(&metadata).is_empty());
    }
}
