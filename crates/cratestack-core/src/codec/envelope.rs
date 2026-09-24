//! The [`CratestackEnvelope`] trait (ADR 0006 §1).

use std::future::Future;

use bytes::Bytes;
use serde::Serialize;

use super::CratestackCodec;
use super::binding::Binding;
use super::stream::{StreamOpener, StreamSealer};
use crate::context::CratestackContext;
use crate::error::CratestackError;

/// Which kind of body an envelope is asked to name a media type for.
///
/// Deliberately **not** `#[non_exhaustive]`. A new shape (a signed
/// subscription binding, ADR 0006 P3) should be a compile error in every
/// envelope, so each one decides its media type on purpose instead of
/// falling into a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyShape {
    /// One body: a unary request or response. Signed, it travels as
    /// `application/cose; cose-type="cose-sign1"` (or `"cose-mac0"`).
    Unary,
    /// A cbor-seq of items: `@stream` responses, and the signed
    /// `/rpc/batch` upload (§7). Signed, it travels as
    /// `application/vnd.cratestack.cose-seq+cbor-seq`.
    Stream,
}

/// The signing seam between the codec and the HTTP body (ADR 0006 §1).
///
/// Replaces the sync, bytes-only `open_request`/`seal_response` trait,
/// which no router ever called. Signing needs a key, the request's context
/// (the [`Binding`]) and `async`, so KMS- and HSM-backed signers can plug
/// in.
///
/// `seal`/`open` return `impl Future + Send` rather than the ADR sketch's
/// `BoxFuture`, which heap-allocates on every call. That would put an
/// allocation on the unsigned path, and cratestack#1004 requires
/// [`NoEnvelope`](super::NoEnvelope) to allocate nothing. Object safety is
/// not lost, because the `Clone` supertrait already rules out `dyn
/// CratestackEnvelope`: routers and clients are generic over `E`. As with
/// [`AuthProvider`](crate::AuthProvider), an implementation may be written
/// as a plain `async fn`.
///
/// Verification failures must be [`CratestackError::Unauthorized`] with a
/// coarse message. The response must never reveal which check failed
/// (§10). A backend outage (the key resolver or the nonce store is
/// unreachable) is not a verification failure: it is
/// [`CratestackError::Internal`], a `500`, logged server-side (ADR 0006, P0
/// scoping decisions).
pub trait CratestackEnvelope: Clone + Send + Sync + 'static {
    /// The `Content-Type` of a sealed body of this shape. `None` means this
    /// envelope adds no framing for `shape`, so the body keeps the
    /// payload's own media type (the codec's `CONTENT_TYPE`, or
    /// `application/cbor-seq`) and content negotiation is unchanged.
    ///
    /// Contract: `media_type(BodyShape::Stream)` is `None` exactly when
    /// [`stream_sealer`](Self::stream_sealer) and
    /// [`stream_opener`](Self::stream_opener) return `None`. A router may
    /// therefore check this first and skip building the owned binding the
    /// stream methods take.
    fn media_type(&self, shape: BodyShape) -> Option<&'static str>;

    /// Wrap `payload`, the codec's output byte for byte, for the peer that
    /// will rebuild `bind`. It runs after `codec.encode`.
    fn seal<'a>(
        &'a self,
        payload: Bytes,
        bind: &'a Binding<'a>,
    ) -> impl Future<Output = Result<Bytes, CratestackError>> + Send + 'a;

    /// Encode `value` with `codec` and seal the result for `bind`: the same
    /// bytes as `codec.encode` followed by [`seal`](Self::seal), which is
    /// exactly what the default does.
    ///
    /// It exists so that an envelope can **encode in place** (ADR 0006 §1;
    /// maintainer decision on cratestack#1005, which kept that design over
    /// accepting one copy of the payload): `cratestack-cose` overrides it to
    /// reserve the payload's `bstr` head inside its output buffer and let
    /// [`CratestackCodec::encode_into`] write the payload right after it, so
    /// the encoded payload is never held in a buffer of its own. `seal`
    /// stays for callers that already hold the encoded bytes.
    ///
    /// A provided method with a default body, added after cratestack#1004
    /// merged the trait, so no implementation had to change. `T: Sync`
    /// because the value is borrowed by the returned `Send` future.
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
        async move {
            let payload = codec.encode(value)?;
            self.seal(Bytes::from(payload), bind).await
        }
    }

    /// Verify `body` against `bind`, run the replay checks, and return the
    /// payload for `codec.decode`. The result should be a zero-copy slice of
    /// `body`, because the verifier checks exactly the bytes it received and
    /// nothing is re-serialized (§1).
    ///
    /// On success an envelope that authenticated a signer records it with
    /// [`CratestackContext::record_verified_signer`]. The router wiring
    /// (cratestack#1006; no router calls this trait yet) lifts that into the
    /// rate-limit and idempotency principal, which is why `open` must run
    /// before both of those layers (§12).
    fn open<'a>(
        &'a self,
        body: Bytes,
        bind: &'a Binding<'a>,
        ctx: &'a mut CratestackContext,
    ) -> impl Future<Output = Result<Bytes, CratestackError>> + Send + 'a;

    /// A sealer for one outgoing stream, or `None` when this envelope does
    /// not sign streams (plain cbor-seq passes through). It takes
    /// `Binding<'static>` because the sealer outlives the request borrow:
    /// chain mode's `h₀ = SHA-256(external_aad)` needs the binding (§6).
    fn stream_sealer(&self, bind: Binding<'static>) -> Option<Box<dyn StreamSealer>>;

    /// An opener for one incoming stream, or `None` when this envelope does
    /// not verify streams. See [`stream_sealer`](Self::stream_sealer).
    fn stream_opener(&self, bind: Binding<'static>) -> Option<Box<dyn StreamOpener>>;
}
