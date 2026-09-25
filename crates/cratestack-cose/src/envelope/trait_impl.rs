//! `CratestackEnvelope` for [`CoseEnvelope`].

use std::future::Future;

use bytes::Bytes;
use cratestack_core::{
    Binding, BodyShape, CratestackCodec, CratestackContext, CratestackEnvelope, CratestackError,
    StreamOpener, StreamSealer, VerifiedSigner,
};
use serde::Serialize;

use super::{CoseEnvelope, CoseRole};
use crate::seal::PAYLOAD_CAPACITY_HINT;

impl CratestackEnvelope for CoseEnvelope {
    /// Unary bodies are `application/cose; cose-type="cose-sign1"` or
    /// `"cose-mac0"`. Streams are `None`: `chain` mode is P1
    /// (cratestack#1008), and until then a stream keeps plain cbor-seq, as
    /// the trait's contract requires when the stream methods return `None`.
    fn media_type(&self, shape: BodyShape) -> Option<&'static str> {
        match shape {
            BodyShape::Unary => Some(self.inner.mode.media_type()),
            BodyShape::Stream => None,
        }
    }

    /// A server seals a response, a client seals a request (see
    /// [`CoseRole`]). The payload arrives encoded, so it is copied into the
    /// message once; [`seal_value`](Self::seal_value) avoids that copy.
    fn seal<'a>(
        &'a self,
        payload: Bytes,
        bind: &'a Binding<'a>,
    ) -> impl Future<Output = Result<Bytes, CratestackError>> + Send + 'a {
        let request = self.inner.role == CoseRole::Client;
        async move {
            crate::seal::seal(&self.inner, bind, request, payload.len(), |out| {
                out.extend_from_slice(&payload);
                Ok(())
            })
            .await
        }
    }

    /// Overridden to encode in place (ADR 0006 §1): `codec.encode_into`
    /// writes the payload straight into the message buffer, after the room
    /// reserved for its `bstr` head, and the head is patched in once the
    /// length is known. The bytes equal `codec.encode` followed by
    /// [`seal`](Self::seal).
    fn seal_value<'a, C, T>(
        &'a self,
        codec: &'a C,
        value: &'a T,
        bind: &'a Binding<'a>,
    ) -> impl Future<Output = Result<Bytes, CratestackError>> + Send + 'a
    where
        C: CratestackCodec,
        T: Serialize + ?Sized + Sync,
    {
        let request = self.inner.role == CoseRole::Client;
        async move {
            crate::seal::seal(&self.inner, bind, request, PAYLOAD_CAPACITY_HINT, |out| {
                codec.encode_into(value, out)
            })
            .await
        }
    }

    /// A server opens a request (replay checks included), a client opens a
    /// response. On success the context records a [`VerifiedSigner`] that
    /// names the key that verified: its thumbprint, its `kid` (the header's,
    /// which the opener checked is the key's own, copied so the context does
    /// not keep the body alive) and the algorithm. The payload returned is a
    /// slice of `body`.
    fn open<'a>(
        &'a self,
        body: Bytes,
        bind: &'a Binding<'a>,
        ctx: &'a mut CratestackContext,
    ) -> impl Future<Output = Result<Bytes, CratestackError>> + Send + 'a {
        let request = self.inner.role == CoseRole::Server;
        async move {
            let opened = crate::open::open(&self.inner, body, bind, request).await?;
            ctx.record_verified_signer(VerifiedSigner::new(
                Bytes::copy_from_slice(&opened.kid),
                opened.thumbprint,
                opened.alg.id(),
            ));
            Ok(opened.payload)
        }
    }

    fn stream_sealer(&self, _bind: Binding<'static>) -> Option<Box<dyn StreamSealer>> {
        None
    }

    fn stream_opener(&self, _bind: Binding<'static>) -> Option<Box<dyn StreamOpener>> {
        None
    }
}
