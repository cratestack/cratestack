//! `CratestackEnvelope` for [`CoseEnvelope`].

use std::future::Future;

use bytes::Bytes;
use cratestack_core::{
    Binding, BodyShape, CratestackContext, CratestackEnvelope, CratestackError, StreamOpener,
    StreamSealer, VerifiedSigner,
};

use super::{CoseEnvelope, CoseRole};

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
    /// [`CoseRole`]). The payload is copied into the message once; see
    /// `seal::seal` for why it cannot be encoded in place here.
    fn seal<'a>(
        &'a self,
        payload: Bytes,
        bind: &'a Binding<'a>,
    ) -> impl Future<Output = Result<Bytes, CratestackError>> + Send + 'a {
        let request = self.inner.role == CoseRole::Client;
        async move { crate::seal::seal(&self.inner, &payload, bind, request).await }
    }

    /// A server opens a request (replay checks included), a client opens a
    /// response. On success the verified `kid` is recorded as the
    /// context's [`VerifiedSigner`], and the payload returned is a slice of
    /// `body`.
    fn open<'a>(
        &'a self,
        body: Bytes,
        bind: &'a Binding<'a>,
        ctx: &'a mut CratestackContext,
    ) -> impl Future<Output = Result<Bytes, CratestackError>> + Send + 'a {
        let request = self.inner.role == CoseRole::Server;
        async move {
            let opened = crate::open::open(&self.inner, body, bind, request).await?;
            ctx.record_verified_signer(VerifiedSigner::new(opened.kid));
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
