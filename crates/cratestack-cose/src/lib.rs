//! `cratestack-cose`: the COSE envelope of ADR 0006, unary part (P0).
//!
//! A signed body is the codec's output wrapped as a COSE_Sign1 (tag 18) or
//! COSE_Mac0 (tag 17), bound to the request through external AAD that is
//! rebuilt on both sides and never sent:
//!
//! ```text
//! typed value ──CborCodec──▶ payload bytes ──CoseEnvelope──▶ COSE_Sign1 / COSE_Mac0
//!                                 ▲                               │
//!                                 └──────── verified as-is ◀──────┘
//! ```
//!
//! - [`CoseEnvelope`] implements `cratestack_core::CratestackEnvelope`, and
//!   also offers typed methods ([`CoseEnvelope::open_request`] and the
//!   rest) that need no `CratestackContext`, for the axum layer
//!   (cratestack#1006), which runs before a context exists.
//! - [`CoseSigner`] and [`CoseVerifierResolver`] are the key seams: a KMS or
//!   HSM signs without exporting the key, and a resolver may return several
//!   candidates for one `kid`.
//! - [`external_aad`] defines the binding (§4), which includes the
//!   receiving service's `audience` (never empty, and distinct from the
//!   audience the service seals its own outbound requests for);
//!   [`request_digest`] and [`request_digest_unsigned`] bind a response to
//!   its request, the latter through the client's [`RequestNonce`]
//!   (`Cratestack-Nonce`), and each returns its digest together with the
//!   `RequestKind` the response AAD binds; [`thumbprint`] the RFC 9679 key
//!   ids (§3).
//! - [`KeyProviderMacKeys`] turns `cratestack_core::KeyProvider` secrets
//!   into Mac0 keys.
//!
//! **Algorithms:** Ed25519 (`-19`, the default) and ESP256 (`-9`) for
//! Sign1, HMAC 256/64 (`4`) and 256/256 (`5`) for Mac0. Nothing else is
//! accepted, including the deprecated `-8` and `-7` (see [`CoseAlg`]).
//!
//! **Errors (§10):** every failed check is the same
//! `CratestackError::Unauthorized` carrying [`UNAUTHENTICATED`]; a failing
//! key resolver, nonce store or signer is `CratestackError::Internal`, and
//! so is local misuse (see [`CoseEnvelope`]).
//!
//! **Not here yet:** `chain` streams (P1), `window` replay (P2), and the
//! `auth` feature (the `cratestack-auth` adapters, the Redis nonce bridge
//! and the enrolment code), which is the second half of cratestack#1005.
//!
//! # Where this departs from the ADR's sketch
//!
//! - The outer structure is emitted and parsed by hand, not with `coset`
//!   (see the private `cbor` module's doc); `coset` is a dev-dependency
//!   that the tests check the bytes against.
//! - Algorithms are the closed [`CoseAlg`], not `coset::iana::Algorithm`.
//! - The payload is encoded in place only on the `seal_value` path
//!   (`CratestackEnvelope::seal_value`, [`CoseEnvelope::seal_request_value`]
//!   and [`CoseEnvelope::seal_response_value`]). `seal` receives bytes that
//!   are already encoded and copies them into the message once.
//! - For in-process signers (HMAC, ESP256 and Ed25519) the signature is
//!   computed over the to-be-signed structure in pieces, with no copy of
//!   the payload; Ed25519 runs both PureEdDSA passes over the pieces (see
//!   [`Ed25519Signer`]). A KMS or HSM signer keeps the default
//!   [`CoseSigner::sign_chunks`] and is handed the structure in one buffer.
//!   Verification never builds it contiguously.
//! - ESP256 signatures are low-`s` only: the sealer normalises them, the
//!   opener rejects a high `s`, so no third party can re-spell a signed
//!   message.

mod aad;
mod alg;
mod cbor;
mod envelope;
mod error;
mod header;
mod keys;
mod open;
mod opened;
mod replay;
mod request_nonce;
mod seal;
mod tbs;
pub mod thumbprint;
mod wire;

pub use aad::{BINDING_VERSION, external_aad, request_digest};
pub use alg::{CoseAlg, CoseMode};
pub use envelope::{CoseEnvelope, CoseEnvelopeBuilder, CoseRole};
pub use error::UNAUTHENTICATED;
pub use keys::{
    CoseSigner, CoseVerifierResolver, CoseVerifyKey, Ed25519Signer, HmacSecret, HmacSigner,
    KeyProviderMacKeys, MIN_HMAC_SECRET_LEN, P256Signer, StaticVerifierResolver,
};
pub use opened::Opened;
pub use replay::{DEFAULT_SKEW_SECS, RANDOM_CTI_LEN};
pub use request_nonce::{
    NONCE_HEADER, NONCE_HEADER_VALUE_LEN, REQUEST_NONCE_LEN, RequestNonce, request_digest_unsigned,
};
