//! Trusted-proxy configuration for the audit `client_ip` (#415):
//! [`TrustedProxyConfig`] plus the tests covering the allowlist/hop-count
//! behavior in isolation from header parsing (see
//! [`crate::headers::forwarded`] for the hop-count-aware chain walk itself
//! and [`crate::headers::enrich_context_from_headers`] for where the two
//! are combined). See `docs/design/trusted-proxy-client-ip.md` for the
//! decided design.

mod config;

pub use config::TrustedProxyConfig;

#[cfg(test)]
mod tests;
