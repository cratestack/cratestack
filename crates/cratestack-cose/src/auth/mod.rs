//! `cratestack-auth` integration: the `auth` Cargo feature (ADR 0006,
//! "Decisions taken while scoping P0"; cratestack#1005 part B).
//!
//! - [`ServiceKeySigner`]: a `cratestack_auth::ServiceSigningKey` as a
//!   [`CoseSigner`](crate::CoseSigner), for service-to-service Sign1.
//! - [`DeviceKeyCoseResolver`]: a `cratestack_auth::DeviceKeyResolver` as a
//!   [`CoseVerifierResolver`](crate::CoseVerifierResolver), for device
//!   requests (§8: "indexed additionally by thumbprint prefix").
//! - [`AuthNonceStore`]: `cratestack_auth`'s nonce store (in-memory or
//!   Redis) as `cratestack_core::NonceStore`, for multi-replica `nonce`
//!   replay protection (§5), keyed by `(kid, cti)`.
//! - [`build_cose_enroll_response`] / [`parse_cose_enroll_response`]: the
//!   enrolment challenge code, moved here from `cratestack-auth` (a
//!   breaking change). It is a separate, older COSE shape and does not go
//!   through the envelope's strict opener (see its module docs).
//!
//! The dependency points one way, `cratestack-cose` -> `cratestack-auth`
//! (L2 -> L1, `docs/adr/layers.toml`). Without the feature this crate
//! depends on `cratestack-core` alone, so clients and the wasm/napi builds
//! stay free of auth's Redis, reqwest and rustls dependencies.
//!
//! The error contract is the envelope's (§10): an adapter never turns a
//! verification failure into anything but the coarse `401`, and a failing
//! backend (the device-key store, the nonce store) is a `500` whose detail
//! stays server-side.

mod device_resolver;
mod enroll;
mod nonce_bridge;
mod service_signer;

pub use device_resolver::DeviceKeyCoseResolver;
pub use enroll::{build_cose_enroll_response, parse_cose_enroll_response};
pub use nonce_bridge::{AUTH_NONCE_KEY_ID, AuthNonceStore};
pub use service_signer::ServiceKeySigner;
