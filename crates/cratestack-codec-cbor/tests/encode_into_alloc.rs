//! `CborCodec::encode_into` writes into the caller's buffer and nothing
//! else (cratestack#1005): with enough capacity reserved it allocates
//! nothing at all, so there is no intermediate `Vec` for a signing envelope
//! to copy out of.
//!
//! A separate test binary because `allocation-counter` installs a counting
//! `#[global_allocator]`; `measure` counts only the calling thread.

use std::hint::black_box;

use allocation_counter::measure;
use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;
use serde::Serialize;

#[derive(Serialize)]
struct Payment<'a> {
    id: [u8; 16],
    payer: &'a str,
    amount: i64,
    currency: &'a str,
    note: Option<&'a str>,
}

const PAYMENT: Payment<'static> = Payment {
    id: [7; 16],
    payer: "Amina Tchoupo",
    amount: 125_000,
    currency: "XAF",
    note: Some("rent sept"),
};

#[test]
fn the_counter_is_live_in_this_binary() {
    let info = measure(|| {
        black_box(CborCodec.encode(&PAYMENT).expect("encode"));
    });
    assert!(info.count_total >= 1, "counting allocator not installed");
}

#[test]
fn encode_into_a_reserved_buffer_allocates_nothing() {
    let expected = CborCodec.encode(&PAYMENT).expect("encode");
    let mut out = Vec::with_capacity(16 + expected.len());
    out.extend_from_slice(b"prefix");
    let buffer = out.as_ptr();
    let info = measure(|| {
        CborCodec
            .encode_into(&PAYMENT, &mut out)
            .expect("encode_into");
    });
    assert_eq!(info.count_total, 0, "encode_into allocated: {info:?}");
    assert_eq!(out.as_ptr(), buffer, "the buffer moved");
    assert_eq!(&out[6..], expected.as_slice());
}
