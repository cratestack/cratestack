//! Axum-bound runtime helpers for cratestack.
//!
//! The crate is organized into themed modules; see each for details:
//!
//! - [`codec`]: codec-bound encode/decode and the [`CodecSet`] pairing.
//! - [`transport`]: the [`HttpTransport`] trait and the
//!   `encode_transport_*` response family.
//! - [`headers`]: request-header helpers (ETag, traceparent, client IP).
//! - [`query`]: URL query parsing and the filter expression grammar.
//! - [`rpc`]: the RPC binding (`POST /rpc/{op_id}` and
//!   `POST /rpc/batch`).
//! - [`idempotency`]: idempotency-key middleware and storage trait.
//! - `middleware_error` (private): the shared, codec-negotiated error
//!   envelope both middleware layers emit (cratestack#846).
//! - [`ratelimit`]: token-bucket rate-limit middleware and storage trait.
//! - [`schema_fingerprint`]: warn-only client/server schema drift
//!   detection via the `x-cratestack-schema-sha` header.
//! - [`trusted_proxy`]: the [`TrustedProxyConfig`] allowlist/hop-count/
//!   [`ForwardedHeader`] type consumers apply as an `Extension` to make
//!   `Forwarded`/`X-Forwarded-For` trust explicit (#415).

pub use axum;

pub mod codec;
pub mod headers;
pub mod idempotency;
mod middleware_error;
pub mod projection;
pub mod query;
pub mod ratelimit;
pub mod rpc;
pub mod schema_fingerprint;
pub mod transport;
pub mod trusted_proxy;

// -----------------------------------------------------------------------------
// Crate-root re-exports — every item the `cratestack-macros` crate references
// from `cratestack_axum::*` must remain importable from the crate root. The
// list below is explicit (not `pub use module::*;`) so the public surface is
// greppable from this file.
// -----------------------------------------------------------------------------

pub use codec::{
    CodecSet, decode_codec_request, encode_codec_response, encode_codec_result,
    encode_codec_result_with_status, validate_codec_request_headers,
    validate_codec_response_headers,
};

pub use transport::{
    CBOR_SEQUENCE_CONTENT_TYPE, HttpTransport, decode_transport_request_for,
    encode_transport_result, encode_transport_result_with_status,
    encode_transport_result_with_status_for, encode_transport_sequence_result,
    encode_transport_sequence_result_with_status, encode_transport_sequence_result_with_status_for,
    encode_transport_stream_result_with_status_for, validate_transport_request_headers,
    validate_transport_request_headers_for, validate_transport_response_headers,
    validate_transport_response_headers_for,
};

pub use headers::{
    ClientIpContext, enrich_context_from_headers, parse_client_ip, parse_if_match_version,
    parse_traceparent, set_version_etag,
};

pub use projection::ProjectedValue;

pub use query::{
    QueryExpr, parse_computed_params_object, parse_filter_expression, parse_query_pairs,
};

pub use trusted_proxy::{ForwardedHeader, TrustedProxyConfig};
