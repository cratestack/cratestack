//! Pluggable codec + envelope traits used by the transport layer.
//!
//! The two are deliberately separate seams (ADR 0001, ADR 0006 §1): a
//! [`CratestackCodec`] turns typed values into payload bytes and needs no
//! key, no request context and no `async`; a [`CratestackEnvelope`] wraps
//! those bytes and needs all three. Processing order is
//! `HTTP body → envelope.open → codec.decode` inbound and
//! `codec.encode → envelope.seal → HTTP body` outbound.

mod binding;
mod bound_headers;
mod envelope;
mod no_envelope;
mod path_params;
mod request_nonce;
mod response_binding;
mod stream;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

use crate::error::CratestackError;

/// The one message every failed envelope verification carries (ADR 0006
/// §10): the peer must never learn which check failed. A malformed body, an
/// unknown `kid`, a disallowed algorithm, a bad signature, a stale `iat` and
/// a replayed `cti` all produce `CratestackError::Unauthorized` with this
/// text, byte for byte.
///
/// Moved here from `cratestack-cose` (which re-exports it) so the axum
/// envelope layer can answer with it without depending on a COSE crate.
pub const UNAUTHENTICATED: &str = "request could not be authenticated";

pub use binding::Binding;
pub use bound_headers::BoundHeaders;
pub use envelope::{BodyShape, CratestackEnvelope};
pub use no_envelope::NoEnvelope;
pub use path_params::PathParams;
pub use request_nonce::{
    NONCE_HEADER, NONCE_HEADER_VALUE_LEN, REQUEST_NONCE_LEN, RequestNonce, request_digest,
    request_digest_unsigned,
};
pub use response_binding::{RequestDigest, RequestKind, ResponseBinding};
pub use stream::{OpenedFrame, SealedItem, StreamEnd, StreamOpener, StreamSealer};

pub trait CratestackCodec: Clone + Send + Sync + 'static {
    const CONTENT_TYPE: &'static str;

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CratestackError>;

    /// Append the encoding of `value` to `out`, leaving what `out` already
    /// holds untouched. On success `out[old_len..]` is byte for byte what
    /// [`encode`](Self::encode) returns.
    ///
    /// This is the hook that lets a signing envelope encode **straight into
    /// its output buffer** (ADR 0006 §1, "the sealer encodes straight into
    /// the COSE buffer"; maintainer decision on cratestack#1005) instead of
    /// encoding into a `Vec` of its own and copying it in. See
    /// [`CratestackEnvelope::seal_value`].
    ///
    /// Provided, so adding it broke no codec: the default encodes and then
    /// copies once, which is correct but is exactly the copy the hook exists
    /// to avoid. A codec whose serializer can write into a `Vec` should
    /// override it (`CborCodec` and `JsonCodec` do). On error an override
    /// should leave `out` at its old length; the default always does.
    fn encode_into<T: Serialize + ?Sized>(
        &self,
        value: &T,
        out: &mut Vec<u8>,
    ) -> Result<(), CratestackError> {
        out.extend_from_slice(&self.encode(value)?);
        Ok(())
    }

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CratestackError>;
}
