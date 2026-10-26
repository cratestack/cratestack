//! Claim objects: a signed payload that outlives a transport envelope.
//!
//! A transport envelope ([`CoseEnvelope`](crate::CoseEnvelope)) is bound to
//! one request and dies with its `iat` skew window and its nonce. An object
//! signed offline by one device and delivered later by another needs the
//! opposite: a self-contained `COSE_Sign1` whose lifetime and replay are
//! the *application's* decision. That is this module:
//!
//! ```text
//! sign_claims(signer, typ, claims, payload, aad) ──▶ tagged COSE_Sign1 ──▶ any channel, any delay
//!                                                          │
//! open_claims(resolver, bytes, typ, aad) ◀─────────────────┘  (signature only; caller checks exp/aud/cti)
//! ```
//!
//! The protected header is `{1: alg, 4: kid, ? 15: CWT claims, 16: typ}`
//! (see `header.rs` for the CDDL); the parser accepts exactly what the
//! signer writes. The mandatory `typ` (RFC 9596) is the domain separator:
//! the envelope's parser refuses label 16 and this one refuses its absence,
//! so neither format opens as the other (`tests/claims_domain_separation.rs`).
//! Algorithms are the envelope's signature algorithms, Ed25519 and ESP256;
//! HMAC / `COSE_Mac0` is refused.
//!
//! Replay: nothing here remembers anything. To make an object single-use,
//! give it a `cti` and record the ones accepted.

mod error;
mod header;
mod open;
mod sign;
mod types;

pub use error::ClaimsError;
pub use open::open_claims;
pub use sign::sign_claims;
pub use types::{CwtClaims, MAX_AUD_LEN, MAX_CTI_LEN, MAX_TYP_LEN, OpenedClaims};
