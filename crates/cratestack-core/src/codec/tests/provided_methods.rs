//! The provided `CratestackCodec::encode_into` and
//! `CratestackEnvelope::seal_value` defaults (cratestack#1005): additive,
//! and byte-identical to the two-step path they shortcut.

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use super::{RoutePrefixEnvelope, poll_once, request_binding};
use crate::codec::{CratestackCodec, CratestackEnvelope, NoEnvelope};
use crate::error::CratestackError;

/// A codec that implements only the required methods, so the provided
/// `encode_into` is the one under test.
#[derive(Clone)]
struct RequiredOnlyCodec;

impl CratestackCodec for RequiredOnlyCodec {
    const CONTENT_TYPE: &'static str = "application/json";

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CratestackError> {
        serde_json::to_vec(value).map_err(|error| CratestackError::Codec(error.to_string()))
    }

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CratestackError> {
        serde_json::from_slice(bytes).map_err(|error| CratestackError::Codec(error.to_string()))
    }
}

#[derive(Serialize)]
struct Row {
    id: u32,
    name: &'static str,
}

const ROW: Row = Row { id: 7, name: "x" };

#[test]
fn default_encode_into_appends_and_keeps_the_prefix() {
    let mut out = b"prefix".to_vec();
    RequiredOnlyCodec
        .encode_into(&ROW, &mut out)
        .expect("encode");
    let encoded = RequiredOnlyCodec.encode(&ROW).expect("encode");
    assert_eq!(&out[..6], b"prefix");
    assert_eq!(&out[6..], encoded.as_slice());
}

/// A value whose `Serialize` fails, to show the default leaves `out` alone.
struct Unserializable;

impl Serialize for Unserializable {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("no"))
    }
}

#[test]
fn default_encode_into_leaves_out_untouched_on_error() {
    let mut out = b"prefix".to_vec();
    assert!(
        RequiredOnlyCodec
            .encode_into(&Unserializable, &mut out)
            .is_err()
    );
    assert_eq!(out, b"prefix");
}

/// Through the generic bound, as a router calls it.
fn seal_value<E: CratestackEnvelope, C: CratestackCodec>(envelope: &E, codec: &C) -> Bytes {
    let bind = request_binding();
    poll_once(envelope.seal_value(codec, &ROW, &bind)).expect("seal_value")
}

#[test]
fn default_seal_value_is_encode_then_seal() {
    let bind = request_binding();
    for (sealed, envelope_two_step) in [
        (
            seal_value(&NoEnvelope, &RequiredOnlyCodec),
            poll_once(NoEnvelope.seal(
                Bytes::from(RequiredOnlyCodec.encode(&ROW).expect("encode")),
                &bind,
            )),
        ),
        (
            seal_value(&RoutePrefixEnvelope, &RequiredOnlyCodec),
            poll_once(RoutePrefixEnvelope.seal(
                Bytes::from(RequiredOnlyCodec.encode(&ROW).expect("encode")),
                &bind,
            )),
        ),
    ] {
        assert_eq!(sealed, envelope_two_step.expect("seal"));
    }
}

#[test]
fn default_seal_value_surfaces_a_codec_error() {
    let bind = request_binding();
    let result = poll_once(NoEnvelope.seal_value(&RequiredOnlyCodec, &Unserializable, &bind));
    assert!(matches!(result, Err(CratestackError::Codec(_))));
}
