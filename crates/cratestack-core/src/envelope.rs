//! Key-resolution and nonce-replay-tracking primitives.
//!
//! [`KeyProvider`] resolves signing keys by `kid`, and [`NonceStore`] records
//! the nonces of signed requests that were already seen so a replay is
//! rejected. They back the COSE envelope in `cratestack-cose`
//! (`KeyProviderMacKeys`, ADR 0006).
//!
//! The HS256 envelope that used to live here was
//! deprecated in 0.19.6 and removed in 0.20.0 (cratestack#1357). COSE_Mac0
//! (`cratestack_cose::CoseEnvelope` in `CoseMode::Mac0`) replaces them.

mod keys;

#[cfg(test)]
mod tests;

pub use keys::{InMemoryNonceStore, KeyProvider, NonceStore, StaticKeyProvider};
