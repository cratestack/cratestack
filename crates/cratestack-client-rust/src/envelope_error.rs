//! Why a signed call failed (ADR 0006, cratestack#1007).

use cratestack_core::CratestackError;

/// A failure that belongs to the signed transport, not to the call.
///
/// A client that has an envelope never falls back to a plain call: every one
/// of these is an error, and none of them carries a decoded body. In
/// particular an answer that arrives without a seal is
/// [`Unsigned`](Self::Unsigned) whatever its status, so a proxy that strips
/// the envelope, or a server that was never asked to sign, cannot turn a
/// `Required` client into a plain one.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EnvelopeError {
    /// The response was not a COSE message. Nothing in its body was read.
    ///
    /// The layer's own refusals (a wrong audience, a stale
    /// `iat`, an unsupported media type, an oversized body) are always
    /// unsigned (ADR 0006 D4), so a `401` here usually means the server did
    /// not accept the request, not that a proxy tampered with the answer.
    #[error("the server answered {status} without a COSE envelope; the body was not read")]
    Unsigned { status: u16 },
    /// The response is a COSE message that failed verification: a bad
    /// signature or tag, an unknown key, or an answer sealed for a different
    /// request, status, route or key. The reason is deliberately not
    /// reported (ADR 0006 §10).
    #[error("the response failed COSE verification")]
    Unverified,
    /// The server answered the unsigned `426` whose body code is
    /// `contract_unsupported` (RPC) or `CONTRACT_UNSUPPORTED` (REST): it no
    /// longer serves the wire shape this client has for `op` (a breaking
    /// change to that op since this client was built), so the call was
    /// refused before any key was looked up. **The answer is unsigned**, so
    /// it is a hint and never proof: anyone on the path could send it, and a
    /// caller should offer "update the app" for that feature, not treat the
    /// server's contract as known. Its body is read only for that code,
    /// which is unauthenticated; a `426` with any other code is
    /// [`Unsigned`](Self::Unsigned). Calls to other ops are unaffected. `op`
    /// is `"METHOD /template"` on REST and the op id on RPC.
    #[error("the server no longer accepts this client's contract for `{op}`; update the client")]
    ContractUnsupported { op: String },
    /// Sealing the request failed: the signer (a keystore, a KMS) refused or
    /// failed, or the envelope was misconfigured. Nothing was sent.
    #[error("sealing the request failed: {0}")]
    Seal(#[source] CratestackError),
    /// Opening the response failed for a reason that is not the message's:
    /// the key resolver's backend is down.
    #[error("opening the response failed: {0}")]
    Open(#[source] CratestackError),
    /// Streams and subscriptions cannot be sealed yet (ADR 0006 P1); the
    /// call was refused locally instead of being sent in plain.
    #[error("sealed streams are not supported yet; call a unary operation")]
    StreamsUnsupported,
}

impl EnvelopeError {
    /// A stable identifier, for a caller that must tell the failures apart
    /// without matching on this enum (the FFI bridge reports it as the
    /// error's `remote_code`).
    pub const fn code(&self) -> &'static str {
        match self {
            EnvelopeError::Unsigned { .. } => "envelope_unsigned",
            EnvelopeError::Unverified => "envelope_unverified",
            EnvelopeError::ContractUnsupported { .. } => "envelope_contract_unsupported",
            EnvelopeError::Seal(_) => "envelope_seal",
            EnvelopeError::Open(_) => "envelope_open",
            EnvelopeError::StreamsUnsupported => "envelope_streams_unsupported",
        }
    }
}
