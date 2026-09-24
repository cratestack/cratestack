//! Pluggable codec + envelope traits used by the transport layer.
//!
//! The two are deliberately separate seams (ADR 0001, ADR 0006 §1): a
//! [`CratestackCodec`] turns typed values into payload bytes and needs no
//! key, no request context and no `async`; a [`CratestackEnvelope`] wraps
//! those bytes and needs all three. Processing order is
//! `HTTP body → envelope.open → codec.decode` inbound and
//! `codec.encode → envelope.seal → HTTP body` outbound.

mod binding;
mod envelope;
mod no_envelope;
mod path_params;
mod stream;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

use crate::error::CratestackError;

pub use binding::Binding;
pub use envelope::{BodyShape, CratestackEnvelope};
pub use no_envelope::NoEnvelope;
pub use path_params::PathParams;
pub use stream::{OpenedFrame, SealedItem, StreamEnd, StreamOpener, StreamSealer};

pub trait CratestackCodec: Clone + Send + Sync + 'static {
    const CONTENT_TYPE: &'static str;

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CratestackError>;

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CratestackError>;
}
