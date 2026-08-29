//! SigV4-style canonical request construction/signing/verification over
//! Ed25519.
//!
//! Split by concern to stay under the repo's 200-LoC-per-file convention:
//! [`types`] (plain data), [`consts`] (env var names + default windows),
//! [`canonical`] (the exact byte string that gets signed), [`keys`]
//! (base64url key/signature encoding), [`validate`] (per-field header
//! checks), [`env`] (parsing the `CRATESTACK_AUTH_SIGNATURE_*` env vars),
//! [`nonce_store`] + [`nonce_redis`] (single-use nonce enforcement),
//! [`device_key_resolver`] (the device-key resolution trait), and
//! [`verifier`] ([`SignedRequestVerifier`] itself).

mod canonical;
mod consts;
mod device_key_resolver;
mod env;
mod keys;
mod nonce_redis;
mod nonce_store;
#[cfg(test)]
mod tests;
mod types;
mod validate;
mod verifier;

pub use canonical::{
    canonical_query, canonical_signature_base, content_sha256_base64url, sign_request,
};
pub use consts::{
    DEFAULT_SIGNATURE_MAX_SKEW_SECONDS, DEFAULT_SIGNATURE_REPLAY_WINDOW_SECONDS,
    SIGNATURE_MAX_SKEW_SECONDS_ENV, SIGNATURE_REPLAY_WINDOW_SECONDS_ENV,
    SIGNATURE_TRUSTED_ISSUERS_ENV, SIGNATURE_TRUSTED_KEYS_ENV,
};
pub use device_key_resolver::DeviceKeyResolver;
pub use keys::{decode_signature_url_safe, decode_verifying_key, encode_verifying_key};
pub use nonce_redis::nonce_store_from_redis_url;
pub use nonce_store::NonceStore;
pub use types::{SignRequestParams, SignedRequestPrincipal};
pub use verifier::SignedRequestVerifier;
