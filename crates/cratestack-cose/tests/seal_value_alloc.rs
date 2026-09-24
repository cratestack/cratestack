//! How many bytes `seal_value` allocates, against the payload's size
//! (cratestack#1005: encode in place, and hash incrementally where the
//! algorithm allows).
//!
//! A separate test binary: `allocation-counter` installs a counting
//! `#[global_allocator]`, and `measure` counts only the calling thread.
//! The futures are polled once with a no-op waker (every signer here is
//! in-process, so they are ready at once), so no runtime allocates on this
//! thread either. `realloc` is counted as a new allocation of the new size.
//!
//! The payload is 256 KiB, so anything payload-sized stands out against
//! the few hundred bytes of headers, AAD and signature. The codec reserves
//! the payload's room up front (see `the_payload_is_encoded_where_it_is_sent`
//! in `seal_value.rs` for why), so the output buffer is allocated once.

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

/// HMAC and ESP256: one payload-sized allocation, the message itself. No
/// buffer of the codec's own, and no contiguous to-be-signed copy.
#[test]
fn hmac_and_esp256_allocate_the_payload_once() {
    let payload = u64::try_from(PAYLOAD).expect("fits");
    for alg in [CoseAlg::Hmac256_64, CoseAlg::Hmac256_256, CoseAlg::Esp256] {
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

/// Ed25519 (PureEdDSA): the message, plus one contiguous copy of the
/// to-be-signed structure, which the `ed25519-dalek` signing API needs.
/// This pins that cost, so it neither grows unnoticed nor is claimed away.
#[test]
fn ed25519_allocates_the_payload_twice() {
    let payload = u64::try_from(PAYLOAD).expect("fits");
    let (info, _) = measure_seal_value(CoseAlg::Ed25519);
    assert!(
        (2 * payload..2 * payload + SLACK).contains(&info.bytes_total),
        "Ed25519: {} bytes for a {PAYLOAD}-byte payload: {info:?}",
        info.bytes_total
    );
}
