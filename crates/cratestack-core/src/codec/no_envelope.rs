//! [`NoEnvelope`], the unsigned default.

use std::future::{Future, ready};

use bytes::Bytes;

use super::binding::Binding;
use super::envelope::{BodyShape, CratestackEnvelope};
use super::stream::{StreamOpener, StreamSealer};
use crate::context::CratestackContext;
use crate::error::CratestackError;

/// Pass-through envelope used when transport-layer signing is not
/// required.
///
/// It is zero-cost. `seal` and `open` return the `Bytes` they were given,
/// the same allocation, through a `std::future::Ready`, so the unsigned
/// path neither copies nor allocates (`tests/no_envelope_alloc.rs` asserts
/// this with a counting allocator). `open` records no signer, because an
/// unsigned body proves nothing about who sent it. Adopting the async trait
/// therefore changes nothing for an existing service.
///
/// `media_type` is `None` for every shape. The previous implementation
/// answered `"application/octet-stream"`, which was wrong for a CBOR body.
/// Nothing called it, so nothing noticed.
#[derive(Debug, Clone, Default)]
pub struct NoEnvelope;

impl CratestackEnvelope for NoEnvelope {
    fn media_type(&self, _shape: BodyShape) -> Option<&'static str> {
        None
    }

    fn seal<'a>(
        &'a self,
        payload: Bytes,
        _bind: &'a Binding<'a>,
    ) -> impl Future<Output = Result<Bytes, CratestackError>> + Send + 'a {
        ready(Ok(payload))
    }

    fn open<'a>(
        &'a self,
        body: Bytes,
        _bind: &'a Binding<'a>,
        _ctx: &'a mut CratestackContext,
    ) -> impl Future<Output = Result<Bytes, CratestackError>> + Send + 'a {
        ready(Ok(body))
    }

    fn stream_sealer(&self, _bind: Binding<'static>) -> Option<Box<dyn StreamSealer>> {
        None
    }

    fn stream_opener(&self, _bind: Binding<'static>) -> Option<Box<dyn StreamOpener>> {
        None
    }
}
