//! cratestack#1315: decoding used to recurse once per nesting level with no
//! bound, so ~8000 `0x81` headers overflowed the stack (SIGSEGV, uncatchable).

use cratestack_codec_cbor::{CborCodec, DEFAULT_MAX_DECODE_DEPTH};
use cratestack_core::CratestackCodec;

/// `depth` one-element arrays wrapped around the integer `1`.
fn nested(depth: usize) -> Vec<u8> {
    let mut bytes = vec![0x81; depth];
    bytes.push(0x01);
    bytes
}

fn decode_json(codec: &impl CratestackCodec, bytes: &[u8]) -> Result<serde_json::Value, String> {
    codec.decode(bytes).map_err(|error| error.to_string())
}

#[test]
fn deep_input_is_an_error_not_a_crash() {
    for depth in [10_000, 100_000, 1_000_000] {
        let error = decode_json(&CborCodec, &nested(depth)).expect_err("must refuse");
        assert!(error.contains("nesting too deep"), "{error}");
    }
}

#[test]
fn deep_input_of_every_container_kind_is_refused() {
    // Indefinite arrays, maps (`a1 01` = {1: ...}), tags (`c1`).
    for header in [
        &[0x9f][..],
        &[0xa1, 0x01][..],
        &[0xc1][..],
        &[0xbf, 0x01][..],
    ] {
        let mut bytes = header.repeat(50_000);
        bytes.push(0x01);
        let error = decode_json(&CborCodec, &bytes).expect_err("must refuse");
        assert!(error.contains("nesting too deep"), "{header:?}: {error}");
    }
}

#[test]
fn default_limit_boundary() {
    let limit = DEFAULT_MAX_DECODE_DEPTH;
    assert_eq!(limit, 128);
    assert!(decode_json(&CborCodec, &nested(limit)).is_ok());
    let error = decode_json(&CborCodec, &nested(limit + 1)).expect_err("limit + 1");
    assert!(error.contains("nesting too deep"), "{error}");
}

#[test]
fn limit_is_configurable_and_boundary_is_exact() {
    let codec = CborCodec::with_max_depth(4);
    assert!(decode_json(&codec, &nested(4)).is_ok());
    assert!(decode_json(&codec, &nested(5)).is_err());
    let wide = CborCodec::with_max_depth(100);
    assert!(decode_json(&wide, &nested(100)).is_ok());
    assert!(decode_json(&wide, &nested(101)).is_err());
    // Depth 0 allows scalars only.
    let flat = CborCodec::with_max_depth(0);
    assert!(decode_json(&flat, &[0x01]).is_ok());
    assert!(decode_json(&flat, &nested(1)).is_err());
}

#[test]
fn codec_stays_usable_after_a_failed_decode() {
    let codec = CborCodec::with_max_depth(8);
    assert!(decode_json(&codec, &nested(10_000)).is_err());
    let good = codec.encode(&vec!["a", "b"]).unwrap();
    let back: Vec<String> = codec.decode(&good).unwrap();
    assert_eq!(back, ["a", "b"]);
    assert!(decode_json(&CborCodec, &nested(10_000)).is_err());
    assert!(decode_json(&CborCodec, &nested(3)).is_ok());
}

#[test]
fn wide_but_shallow_input_is_not_limited() {
    // 100k siblings at depth 1.
    let mut bytes = vec![0x9a, 0x00, 0x01, 0x86, 0xa0];
    bytes.extend(std::iter::repeat_n(0x01, 100_000));
    assert!(decode_json(&CborCodec::with_max_depth(1), &bytes).is_ok());
}

#[test]
fn nested_siblings_do_not_accumulate_depth() {
    // [[1],[1],[1]] has depth 2 however many siblings follow.
    let bytes = [0x83, 0x81, 0x01, 0x81, 0x01, 0x81, 0x01];
    assert!(decode_json(&CborCodec::with_max_depth(2), &bytes).is_ok());
    assert!(decode_json(&CborCodec::with_max_depth(1), &bytes).is_err());
}

#[test]
fn malformed_input_still_reports_the_decoder_error() {
    let error = decode_json(&CborCodec, &[0x81]).expect_err("truncated");
    assert!(!error.contains("nesting too deep"), "{error}");
}

/// The default must fit comfortably in a small thread stack in a debug build
/// (the worst case: serde_json::Value plus path tracking per level).
#[test]
fn default_limit_fits_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            assert!(decode_json(&CborCodec, &nested(DEFAULT_MAX_DECODE_DEPTH)).is_ok());
        })
        .unwrap()
        .join()
        .unwrap();
}
