//! `NoEnvelope` pass-through, `Binding`, and the documented implementor
//! ergonomics of `CratestackEnvelope`. The allocation guarantee lives in
//! `tests/no_envelope_alloc.rs`, which needs its own `#[global_allocator]`.

mod stream_shape;

use std::borrow::Cow;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use bytes::Bytes;

use super::{Binding, BodyShape, CratestackEnvelope, NoEnvelope};
use crate::context::{CratestackContext, VerifiedSigner};
use crate::error::CratestackError;

fn request_binding() -> Binding<'static> {
    Binding {
        method: Cow::Borrowed("POST"),
        route: Cow::Borrowed("model.Payment.create"),
        query: None,
        schema_sha: [7; 32],
        payload_media_type: Cow::Borrowed("application/cbor"),
        request_digest: None,
        status: None,
    }
}

/// `NoEnvelope`'s futures are `Ready`: one poll, no runtime.
fn poll_once<F: Future>(fut: F) -> F::Output {
    match pin!(fut).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("NoEnvelope future was not immediately ready"),
    }
}

#[test]
fn no_envelope_seal_returns_the_same_allocation() {
    let payload = Bytes::from(vec![0xa1, 0x61, 0x61, 0x01]);
    let (ptr, len) = (payload.as_ptr(), payload.len());
    let sealed = poll_once(NoEnvelope.seal(payload, &request_binding())).expect("seal");
    assert_eq!((sealed.as_ptr(), sealed.len()), (ptr, len));
}

#[test]
fn no_envelope_open_returns_the_same_allocation_and_records_no_signer() {
    let body = Bytes::from(vec![0xa1, 0x61, 0x61, 0x01]);
    let (ptr, len) = (body.as_ptr(), body.len());
    let mut ctx = CratestackContext::anonymous();
    let opened = poll_once(NoEnvelope.open(body, &request_binding(), &mut ctx)).expect("open");
    assert_eq!((opened.as_ptr(), opened.len()), (ptr, len));
    assert_eq!(
        ctx,
        CratestackContext::anonymous(),
        "open must not touch ctx"
    );
    assert!(ctx.verified_signer().is_none());
}

#[test]
fn no_envelope_adds_no_framing_and_no_stream_signing() {
    assert_eq!(NoEnvelope.media_type(BodyShape::Unary), None);
    assert_eq!(NoEnvelope.media_type(BodyShape::Stream), None);
    // The trait contract: `media_type(Stream)` is `None` exactly when both
    // stream methods are.
    assert!(NoEnvelope.stream_sealer(request_binding()).is_none());
    assert!(NoEnvelope.stream_opener(request_binding()).is_none());
}

#[test]
fn into_owned_keeps_every_field_and_detaches_the_borrow() {
    let route = String::from("model.Payment.refund");
    let query = String::from("a=1&b=2");
    let borrowed = Binding {
        method: Cow::Borrowed("POST"),
        route: Cow::Borrowed(&route),
        query: Some(Cow::Borrowed(&query)),
        schema_sha: [9; 32],
        payload_media_type: Cow::Borrowed("application/cbor"),
        request_digest: Some([3; 32]),
        status: Some(201),
    };
    let owned: Binding<'static> = borrowed.into_owned();
    // Outliving the strings it borrowed from is the point of `into_owned`.
    drop((route, query));
    assert!(matches!(owned.route, Cow::Owned(_)));
    assert_eq!(owned.route, "model.Payment.refund");
    assert!(matches!(owned.query, Some(Cow::Owned(_))));
    assert_eq!(owned.method, "POST");
    assert_eq!(owned.query.as_deref(), Some("a=1&b=2"));
    assert_eq!(owned.schema_sha, [9; 32]);
    assert_eq!(owned.payload_media_type, "application/cbor");
    assert_eq!(owned.request_digest, Some([3; 32]));
    assert_eq!(owned.status, Some(201));
}

/// A toy envelope written with plain `async fn`, as the trait docs promise
/// an implementor may. It "signs" by prefixing the route, which is enough to
/// exercise the binding and the signer-recording contract. It is not
/// cryptography.
#[derive(Clone)]
struct RoutePrefixEnvelope;

impl CratestackEnvelope for RoutePrefixEnvelope {
    fn media_type(&self, shape: BodyShape) -> Option<&'static str> {
        match shape {
            BodyShape::Unary => Some("application/x-test-sealed"),
            BodyShape::Stream => None,
        }
    }

    async fn seal(&self, payload: Bytes, bind: &Binding<'_>) -> Result<Bytes, CratestackError> {
        Ok([bind.route.as_bytes(), b"\0", &payload].concat().into())
    }

    async fn open(
        &self,
        body: Bytes,
        bind: &Binding<'_>,
        ctx: &mut CratestackContext,
    ) -> Result<Bytes, CratestackError> {
        let prefix_len = bind.route.len() + 1;
        if body.len() < prefix_len || &body[..bind.route.len()] != bind.route.as_bytes() {
            return Err(CratestackError::Unauthorized("invalid envelope".to_owned()));
        }
        ctx.record_verified_signer(VerifiedSigner::new(&b"toy-kid"[..]));
        Ok(body.slice(prefix_len..))
    }

    fn stream_sealer(&self, _bind: Binding<'static>) -> Option<Box<dyn super::StreamSealer>> {
        None
    }

    fn stream_opener(&self, _bind: Binding<'static>) -> Option<Box<dyn super::StreamOpener>> {
        None
    }
}

/// Routers are generic over `E`; go through that path, not the inherent one.
async fn round_trip<E: CratestackEnvelope>(
    envelope: &E,
    seal_for: &Binding<'_>,
    open_as: &Binding<'_>,
    ctx: &mut CratestackContext,
) -> Result<Bytes, CratestackError> {
    let sealed = envelope
        .seal(Bytes::from_static(b"payload"), seal_for)
        .await?;
    envelope.open(sealed, open_as, ctx).await
}

#[tokio::test]
async fn async_fn_impl_satisfies_the_trait_and_records_the_signer() {
    let bind = request_binding();
    let mut ctx = CratestackContext::anonymous();
    let payload = round_trip(&RoutePrefixEnvelope, &bind, &bind, &mut ctx).await;
    assert_eq!(payload.expect("open"), Bytes::from_static(b"payload"));
    assert_eq!(
        ctx.verified_signer().map(|s| s.kid()),
        Some(&b"toy-kid"[..])
    );
    // A recorded signer is not an authenticated identity (#1006 decides).
    assert!(!ctx.is_authenticated());
}

#[tokio::test]
async fn a_body_sealed_for_another_route_is_rejected_without_a_signer() {
    let mut refund = request_binding();
    refund.route = Cow::Borrowed("model.Payment.refund");
    let mut ctx = CratestackContext::anonymous();
    let err = round_trip(&RoutePrefixEnvelope, &request_binding(), &refund, &mut ctx).await;
    assert!(matches!(err, Err(CratestackError::Unauthorized(_))));
    assert!(ctx.verified_signer().is_none());
}
