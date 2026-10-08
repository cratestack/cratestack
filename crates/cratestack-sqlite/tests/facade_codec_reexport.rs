//! A consumer can name the wire codecs through the facade alone, with no
//! direct `cratestack-codec-cbor` / `cratestack-codec-json` dependency
//! in its own manifest. The facade's `[lib]` is named `cratestack`, so the
//! consumer-facing paths are `cratestack::CborCodec` and
//! `cratestack::JsonCodec`. They must be the same types the client runtime
//! (`cratestack::client_rust`) and the codec crates use.

use std::any::TypeId;

use cratestack::{CborCodec, CratestackCodec};

#[test]
fn cbor_codec_is_nameable_and_round_trips_through_the_facade() {
    let bytes = CborCodec.encode(&vec![1_u32, 2, 3]).expect("encode");
    let back: Vec<u32> = CborCodec.decode(&bytes).expect("decode");
    assert_eq!(back, vec![1, 2, 3]);
    assert_eq!(
        <CborCodec as CratestackCodec>::CONTENT_TYPE,
        "application/cbor"
    );
}

#[test]
fn facade_codecs_are_the_client_runtime_codecs() {
    assert_eq!(
        TypeId::of::<CborCodec>(),
        TypeId::of::<cratestack::client_rust::CborCodec>()
    );
}

#[cfg(feature = "codec-json")]
#[test]
fn json_codec_is_nameable_through_the_facade() {
    use cratestack::JsonCodec;

    let bytes = JsonCodec.encode(&vec![1_u32, 2]).expect("encode");
    assert_eq!(bytes, b"[1,2]");
    assert_eq!(
        TypeId::of::<JsonCodec>(),
        TypeId::of::<cratestack::client_rust::JsonCodec>()
    );
}
