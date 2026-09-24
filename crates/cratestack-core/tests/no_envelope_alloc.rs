//! `NoEnvelope` allocates nothing on the unsigned path (cratestack#1004).
//!
//! This is a separate test binary because it links `allocation-counter`,
//! which installs a counting `#[global_allocator]`. The workspace forbids
//! `unsafe_code`, so the `unsafe impl GlobalAlloc` cannot live here.
//! `measure` counts only the calling thread's allocations, so the libtest
//! harness's other threads cannot disturb a count. The futures are polled
//! once with a no-op waker, so no runtime allocates on this thread either.
//!
//! The payload is a heap `Bytes` built *before* measuring. "Zero-copy"
//! means handing that same `Bytes` back. Cloning a `Vec`-backed `Bytes` can
//! itself allocate (promotion to a shared buffer), so nothing is cloned
//! inside a measured region.

use std::borrow::Cow;
use std::future::Future;
use std::hint::black_box;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use allocation_counter::measure;
use bytes::Bytes;
use cratestack_core::{
    Binding, BodyShape, CratestackContext, CratestackEnvelope, NoEnvelope, PathParams,
};

/// Every field borrowed, the way a router builds a unary binding. REST
/// path parameters come from a stack array, as generated code that knows
/// the route's parameter count can build them; RPC passes `EMPTY`.
fn binding<'a>(route: &'a str, path_params: &'a [&'a str], query: Option<&'a str>) -> Binding<'a> {
    Binding {
        method: Cow::Borrowed("POST"),
        route: Cow::Borrowed(route),
        path_params: PathParams::Borrowed(path_params),
        query: query.map(Cow::Borrowed),
        schema_sha: [7; 32],
        payload_media_type: Cow::Borrowed("application/cbor"),
        request_digest: None,
        status: None,
    }
}

fn poll_once<F: Future>(fut: F) -> F::Output {
    match pin!(fut).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("NoEnvelope future was not immediately ready"),
    }
}

/// Go through the generic bound, as a router does, not the concrete type.
fn seal<E: CratestackEnvelope>(envelope: &E, payload: Bytes, bind: &Binding<'_>) -> Bytes {
    poll_once(envelope.seal(payload, bind)).expect("seal")
}

fn open<E: CratestackEnvelope>(
    envelope: &E,
    body: Bytes,
    bind: &Binding<'_>,
    ctx: &mut CratestackContext,
) -> Bytes {
    poll_once(envelope.open(body, bind, ctx)).expect("open")
}

fn heap_body() -> Bytes {
    Bytes::from(vec![0xa2, 0x61, 0x61, 0x01, 0x61, 0x62, 0x02])
}

/// Guards against a vacuous pass. If the counting allocator were not the
/// global allocator in this binary, every count below would read 0.
#[test]
fn the_counter_is_live_in_this_binary() {
    let info = measure(|| {
        black_box(vec![0_u8; 16]);
    });
    assert!(
        info.count_total >= 1,
        "counting allocator not installed: {info:?}"
    );
}

#[test]
fn no_envelope_seal_does_not_allocate() {
    let route = String::from("/accounts/{id}/payments");
    let account = String::from("acc-1");
    let payload = heap_body();
    let (ptr, len) = (payload.as_ptr(), payload.len());
    let mut sealed = None;
    let info = measure(|| {
        let params = [account.as_str()];
        let bind = binding(&route, &params, Some("a=1"));
        sealed = Some(seal(&NoEnvelope, payload, &bind));
    });
    assert_eq!(info.count_total, 0, "NoEnvelope::seal allocated: {info:?}");
    let sealed = sealed.expect("measured closure ran");
    assert_eq!((sealed.as_ptr(), sealed.len()), (ptr, len), "not zero-copy");
}

#[test]
fn no_envelope_open_does_not_allocate() {
    let route = String::from("model.Payment.create");
    let body = heap_body();
    let (ptr, len) = (body.as_ptr(), body.len());
    let mut ctx = CratestackContext::anonymous();
    let mut opened = None;
    let info = measure(|| {
        let bind = binding(&route, &[], None);
        opened = Some(open(&NoEnvelope, body, &bind, &mut ctx));
    });
    assert_eq!(info.count_total, 0, "NoEnvelope::open allocated: {info:?}");
    let opened = opened.expect("measured closure ran");
    assert_eq!((opened.as_ptr(), opened.len()), (ptr, len), "not zero-copy");
    assert!(ctx.verified_signer().is_none());
}

/// The stream path with `NoEnvelope` costs nothing either, as long as the
/// router builds the `Binding<'static>` from `'static` parts (the generated
/// `op_id`, the codec's `CONTENT_TYPE`) or consults `media_type` first.
#[test]
fn no_envelope_media_type_and_stream_methods_do_not_allocate() {
    let mut answers = None;
    let info = measure(|| {
        answers = Some((
            NoEnvelope.media_type(BodyShape::Unary),
            NoEnvelope.media_type(BodyShape::Stream),
            NoEnvelope
                .stream_sealer(binding("model.Payment.list", &[], None))
                .is_none(),
            NoEnvelope
                .stream_opener(binding("model.Payment.list", &[], None))
                .is_none(),
        ));
    });
    assert_eq!(
        info.count_total, 0,
        "NoEnvelope stream path allocated: {info:?}"
    );
    assert_eq!(answers, Some((None, None, true, true)));
}

/// An RPC binding detached for a stream keeps its (empty) path parameters
/// without allocating: `into_owned` of no values is an empty `Vec`.
#[test]
fn empty_path_params_into_owned_does_not_allocate() {
    let mut owned = None;
    let info = measure(|| {
        owned = Some(PathParams::EMPTY.into_owned());
    });
    assert_eq!(info.count_total, 0, "empty PathParams allocated: {info:?}");
    assert!(owned.is_some_and(|params| params.is_empty()));
}
