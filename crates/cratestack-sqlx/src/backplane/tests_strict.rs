//! What the readers refuse (an ambiguous envelope is skipped, never guessed
//! at) and what they ignore (a nonce, of any type), plus the by-reference
//! size check.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::notify_payload::{Body, CBOR_PREFIX, Envelope, decode, encode_text};

fn cbor(bytes: &[u8]) -> String {
    format!("{CBOR_PREFIX}{}", STANDARD.encode(bytes))
}

/// `{"v": 1, "c": "c"}` and then `extra` entries, `entries` in all.
fn map(entries: u8, extra: &[&[u8]]) -> Vec<u8> {
    let mut out = vec![0xa0 | entries, 0x61, b'v', 0x01, 0x61, b'c', 0x61, b'c'];
    out.extend(extra.iter().flat_map(|e| e.iter().copied()));
    out
}

const T: &[u8] = &[0x61, b't', 0x61, b'x'];
const B: &[u8] = &[0x61, b'b', 0x41, 0x01];

#[test]
fn a_cbor_envelope_that_says_one_thing_twice_is_refused() {
    // two payload keys
    let both = decode(&cbor(&map(4, &[T, B]))).unwrap_err();
    assert!(both.contains("payload"), "{both}");
    // the same payload key twice
    let twice = decode(&cbor(&map(4, &[T, T]))).unwrap_err();
    assert!(
        twice.contains("duplicate") || twice.contains("payload"),
        "{twice}"
    );
    // `v` twice (the last one would have won)
    let v_twice = [
        0xa4, 0x61, b'v', 0x02, 0x61, b'v', 0x01, 0x61, b'c', 0x61, b'c', 0x61, b't', 0x61, b'x',
    ];
    assert!(decode(&cbor(&v_twice)).unwrap_err().contains("duplicate"));
    // `c` twice
    let c_twice = [
        0xa4, 0x61, b'v', 0x01, 0x61, b'c', 0x61, b'a', 0x61, b'c', 0x61, b'b', 0x61, b't', 0x61,
        b'x',
    ];
    assert!(decode(&cbor(&c_twice)).unwrap_err().contains("duplicate"));
    // one of each is fine
    assert!(decode(&cbor(&map(3, &[T]))).is_ok());
}

#[test]
fn a_cbor_envelope_with_anything_after_the_map_is_refused() {
    let mut trailing = map(3, &[T]);
    trailing.extend([0x00]);
    let reason = decode(&cbor(&trailing)).unwrap_err();
    assert!(reason.contains("trailing"), "{reason}");
}

#[test]
fn a_json_envelope_with_two_payloads_is_refused() {
    for raw in [
        r#"{"c":"c","p":"x","b":"AQ=="}"#,
        r#"{"c":"c","p":"x","r":"00000000-0000-0000-0000-000000000001"}"#,
        r#"{"c":"c","r":"00000000-0000-0000-0000-000000000001","b":"AQ=="}"#,
    ] {
        let reason = decode(raw).unwrap_err();
        assert!(reason.contains("more than one"), "{raw}: {reason}");
    }
}

#[test]
fn leading_whitespace_is_allowed_as_0_20_5_allowed_it() {
    let json = decode(" \n\t{\"c\":\"c\",\"p\":\"hi\"}").unwrap();
    assert_eq!(json.body, Body::Text("hi".into()));
    let envelope = encode_text(Envelope::Cbor, "c", "hi", None).unwrap();
    assert_eq!(
        decode(&format!(" {envelope}")).unwrap().body,
        Body::Text("hi".into())
    );
}

#[test]
fn a_nonce_of_any_type_or_sign_is_ignored() {
    // `n` as -1, as a float, as text, as a big unsigned
    for nonce in [
        &[0x20][..],
        &[0xf9, 0x3c, 0x00],
        &[0x61, b'z'],
        &[0x1b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
    ] {
        let n = [&[0x61, b'n'][..], nonce].concat();
        let raw = cbor(&map(4, &[T, &n]));
        let read = decode(&raw).unwrap();
        assert_eq!(read.body, Body::Text("x".into()));
    }
    let read = decode(r#"{"c":"c","p":"x","n":-5}"#).unwrap();
    assert_eq!(read.body, Body::Text("x".into()));
}

#[test]
fn a_reference_that_still_does_not_fit_is_reported_not_sent() {
    // The channel alone is over the limit, so the by-reference form is too.
    let channel = "c".repeat(8000);
    let id = uuid::Uuid::from_u128(1);
    for envelope in [Envelope::Cbor, Envelope::Json] {
        assert!(encode_text(envelope, &channel, "{}", Some(id)).is_none());
        assert!(encode_text(envelope, &channel, &"x".repeat(9000), Some(id)).is_none());
    }
}
