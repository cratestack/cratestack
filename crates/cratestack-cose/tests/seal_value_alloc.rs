//! How many bytes `seal_value` allocates, against the payload's size
//! (cratestack#1005: encode in place, and compute every in-process
//! signature over the to-be-signed structure in pieces, Ed25519 included
//! since the 2026-09-25 decision).
//!
//! A separate test binary: `allocation-counter` installs a counting
//! `#[global_allocator]`, and `measure` counts only the calling thread.
//! The futures are polled once with a no-op waker (every signer here is
//! in-process, so they are ready at once), so no runtime allocates on this
//! thread either. `realloc` is counted as a new allocation of the new size.
//!
//! The payload is 256 KiB, so anything payload-sized stands out against
//! the few hundred bytes of headers, AAD and signature.
//!
//! **This measures the best case**: the test codec reserves the payload's
//! room before it writes (see `the_payload_is_encoded_where_it_is_sent` in
//! `seal_value.rs` for why), so the output buffer is allocated once and
//! never grows. A codec that does not reserve, like `CborCodec` itself,
//! grows the buffer as it writes, which the allocator may do by moving it,
//! the same growth its own `Vec` would go through; `realloc` then counts
//! each new size. What this pins is that the envelope adds no
//! payload-sized buffer of its own: no second copy for the codec, and no
//! contiguous to-be-signed copy for any algorithm.

mod common;
#[path = "common/in_place.rs"]
mod in_place;

use std::future::Future;
use std::hint::black_box;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use allocation_counter::{AllocationInfo, measure};
use bytes::Bytes;
use common::{CTI_16, IAT, rpc_request};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CratestackCodec, CratestackEnvelope};
use cratestack_cose::CoseAlg;
use in_place::{RecordingCbor, Text};

const PAYLOAD: usize = 256 * 1024;
/// Everything that is not payload-sized: the headers, the AAD, the
/// signature, the `Bytes` bookkeeping, a boxed signer future.
const SLACK: u64 = 8 * 1024;

fn poll_once<F: Future>(future: F) -> F::Output {
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("an in-process seal was not ready at once"),
    }
}

fn measure_seal_value(alg: CoseAlg) -> (AllocationInfo, Bytes) {
    let codec = RecordingCbor {
        reserve: PAYLOAD + 256,
        ..RecordingCbor::default()
    };
    let value = Text::of(PAYLOAD);
    let envelope = common::client(alg, IAT, CTI_16);
    let bind = rpc_request();
    let mut sealed = None;
    let info = measure(|| {
        sealed = Some(poll_once(envelope.seal_value(&codec, &value, &bind)).expect("seal_value"));
    });
    (info, sealed.expect("ran"))
}

fn measure_encode_then_seal(alg: CoseAlg) -> AllocationInfo {
    let value = Text::of(PAYLOAD);
    let envelope = common::client(alg, IAT, CTI_16);
    let bind = rpc_request();
    measure(|| {
        let payload = CborCodec.encode(&value).expect("encode");
        black_box(poll_once(envelope.seal(Bytes::from(payload), &bind)).expect("seal"));
    })
}

#[test]
fn the_counter_is_live_in_this_binary() {
    let info = measure(|| {
        black_box(vec![0_u8; 16]);
    });
    assert!(info.count_total >= 1, "counting allocator not installed");
}

/// Every algorithm: one payload-sized allocation, the message itself. No
/// buffer of the codec's own, and no contiguous to-be-signed copy, Ed25519
/// included (it allocated the payload twice before its signing was
/// streamed).
#[test]
fn every_algorithm_allocates_the_payload_once() {
    let payload = u64::try_from(PAYLOAD).expect("fits");
    for &alg in CoseAlg::ALL {
        let (info, sealed) = measure_seal_value(alg);
        assert!(sealed.len() > PAYLOAD);
        assert!(
            info.bytes_total < payload + SLACK,
            "{alg:?}: {} bytes allocated for a {PAYLOAD}-byte payload: {info:?}",
            info.bytes_total
        );
        // The two-step path holds the payload twice (the codec's buffer,
        // then the message), which is what the hook removes.
        let two_step = measure_encode_then_seal(alg);
        assert!(two_step.bytes_total >= 2 * payload, "{alg:?}: {two_step:?}");
    }
}

/// A server opening what it was sent allocates nothing payload-sized
/// either: the payload is a slice of the body, and verification (Ed25519
/// included) reads the to-be-signed structure in pieces.
#[test]
fn opening_allocates_nothing_payload_sized() {
    let payload = u64::try_from(PAYLOAD).expect("fits");
    for &alg in CoseAlg::ALL {
        let (_, sealed) = measure_seal_value(alg);
        let server = common::server(alg, IAT);
        let bind = rpc_request();
        let mut opened = None;
        let info = measure(|| {
            opened = Some(poll_once(server.open_request(sealed.clone(), &bind)));
        });
        opened.expect("ran").expect("opens");
        assert!(
            info.bytes_total < SLACK,
            "{alg:?}: {} bytes allocated opening a {payload}-byte payload: {info:?}",
            info.bytes_total
        );
    }
}
