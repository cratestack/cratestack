//! #1153: `CborCodec::decode` must refuse bytes after the first item, the way
//! `JsonCodec::decode` refuses trailing characters.

use cratestack_core::{CratestackCodec, CratestackError};

use super::CborCodec;

fn codec_detail<T: std::fmt::Debug>(result: Result<T, CratestackError>) -> String {
    let error = result.expect_err("trailing bytes must be refused");
    assert!(
        matches!(error, CratestackError::Codec(_)),
        "expected a Codec error, got {error:?}"
    );
    error
        .detail()
        .expect("codec error carries a detail")
        .to_owned()
}

#[test]
fn u32_with_trailing_garbage_is_refused() {
    // `01` is the item; `ff ff de ad` is not part of it.
    let detail = codec_detail(CborCodec.decode::<u32>(&[0x01, 0xff, 0xff, 0xde, 0xad]));
    assert_eq!(
        detail,
        "failed to decode CBOR body: 4 bytes left unconsumed after the value"
    );
}

#[test]
fn vec_with_trailing_items_is_refused() {
    // `82 01 02` is the array `[1, 2]`; `f6 f6` are two more items.
    let detail = codec_detail(CborCodec.decode::<Vec<u32>>(&[0x82, 0x01, 0x02, 0xf6, 0xf6]));
    assert_eq!(
        detail,
        "failed to decode CBOR body: 2 bytes left unconsumed after the value"
    );
}

#[test]
fn a_single_trailing_byte_is_refused() {
    let detail = codec_detail(CborCodec.decode::<u32>(&[0x01, 0x00]));
    assert_eq!(
        detail,
        "failed to decode CBOR body: 1 byte left unconsumed after the value"
    );
}

#[test]
fn a_complete_second_item_is_refused() {
    // Two whole items: the first decodes, the second is still trailing.
    let detail = codec_detail(CborCodec.decode::<u32>(&[0x01, 0x02]));
    assert_eq!(
        detail,
        "failed to decode CBOR body: 1 byte left unconsumed after the value"
    );
}

#[test]
fn a_clean_item_still_decodes() {
    assert_eq!(CborCodec.decode::<u32>(&[0x01]).expect("u32"), 1);
    assert_eq!(
        CborCodec
            .decode::<Vec<u32>>(&[0x82, 0x01, 0x02])
            .expect("vec"),
        vec![1, 2]
    );
}

#[test]
fn a_clean_item_with_an_indefinite_length_array_still_decodes() {
    // `9f 01 02 ff` is `[_ 1, 2]`; the break byte belongs to the item, so
    // nothing is left over.
    assert_eq!(
        CborCodec
            .decode::<Vec<u32>>(&[0x9f, 0x01, 0x02, 0xff])
            .expect("indefinite array"),
        vec![1, 2]
    );
}

#[test]
fn a_unit_variant_sent_as_a_one_entry_map_is_refused() {
    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    enum E {
        A,
        B(u32),
    }
    // `a1 61 41 f6` is `{"A": null}`. minicbor-serde 0.7.1's `unit_variant`
    // does not consume the map value, so the trailing `f6` is left over even
    // though it is inside the item. A first-party encoder sends unit variants
    // as plain strings, so refusing this form loses nothing legitimate.
    let detail = codec_detail(CborCodec.decode::<E>(&[0xa1, 0x61, 0x41, 0xf6]));
    assert_eq!(
        detail,
        "failed to decode CBOR body: 1 byte left unconsumed after the value"
    );
}
