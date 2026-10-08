//! The COSE signed transport behind `flutter_rust_bridge` (cratestack#1026,
//! ADR 0006 §11): Dart seals requests and opens responses with
//! `cratestack-cose`, the one implementation, instead of re-implementing
//! it. Off by default (`cose`, implied by `frb-glue`), so a default build of
//! this crate stays free of crypto.
//!
//! ```text
//! Dart ──FlutterCallBinding──▶ CallBinding ──▶ Binding ──▶ CoseEnvelope
//!   ▲                          (canonical query, AAD inputs: cratestack-cose)
//!   └────── bytes / FlutterOpened / FlutterCoseError ◀──────────┘
//! ```
//!
//! - [`envelope`]: the opaque [`FlutterClientEnvelope`].
//! - [`types`]: the plain values that cross the bridge.
//! - [`error`]: [`FlutterCoseError`], where every failed verification is
//!   the same `Rejected`.
//!
//! Only Required mode exists on the client, as in the Rust client: a client
//! that cannot seal does not call.

pub mod envelope;
pub mod error;
pub mod types;

pub use envelope::FlutterClientEnvelope;
pub use error::{FlutterCoseError, FlutterCoseErrorKind};
pub use types::{
    FlutterCallBinding, FlutterCoseAlg, FlutterCoseMode, FlutterOpened, FlutterSealOptions,
    FlutterServerKey,
};

/// The `Cratestack-Contract` header value for an op contract digest (11
/// characters of unpadded base64url). The header is unbound; send it with
/// every sealed call.
#[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync))]
pub fn cose_contract_header_value(contract_sha: Vec<u8>) -> Result<String, FlutterCoseError> {
    let digest: [u8; 32] = contract_sha
        .try_into()
        .map_err(|_| FlutterCoseError::misuse("the op contract digest is 32 bytes"))?;
    Ok(cratestack_cose::contract_header_value(&digest))
}
