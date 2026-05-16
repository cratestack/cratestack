//! Header helpers used by axum-bound handlers: optimistic-locking ETag
//! parsing/emission, W3C `traceparent` extraction, RFC 7239 `Forwarded`
//! client-IP extraction, and context enrichment that bundles those.

mod enrich;
mod etag;
mod forwarded;
mod traceparent;

pub use enrich::enrich_context_from_headers;
pub use etag::{parse_if_match_version, set_version_etag};
pub use forwarded::parse_client_ip;
pub use traceparent::parse_traceparent;

#[cfg(test)]
mod tests_correlation;
#[cfg(test)]
mod tests_if_match;
