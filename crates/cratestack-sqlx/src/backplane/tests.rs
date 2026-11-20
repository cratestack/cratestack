use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;

use super::notify_payload::{
    Body, CBOR_PREFIX, Envelope, NOTIFY_LIMIT, decode, encode_bytes, encode_text,
};

fn text(raw: &str) -> String {
    match decode(raw).unwrap().body {
        Body::Text(text) => text.into_owned(),
        other => panic!("not text: {other:?}"),
    }
}

#[test]
fn a_large_text_is_sent_by_reference_when_it_can_be() {
    for envelope in [Envelope::Cbor, Envelope::Json] {
        let small = encode_text(envelope, "c", "{}", None).unwrap();
        assert_eq!(text(&small), "{}");
        let big = "x".repeat(NOTIFY_LIMIT);
        let id = uuid::Uuid::from_u128(7);
        let by_ref = encode_text(envelope, "c", &big, Some(id)).unwrap();
        assert!(by_ref.len() <= NOTIFY_LIMIT);
        assert_eq!(decode(&by_ref).unwrap().body, Body::Outbox(id));
        assert!(encode_text(envelope, "c", &big, None).is_none());
    }
}

#[test]
fn the_cbor_envelope_is_marked_and_the_json_one_is_not() {
    let cbor = encode_text(Envelope::Cbor, "ch", "hi", None).unwrap();
    assert!(cbor.starts_with(CBOR_PREFIX) && !cbor.starts_with('{'));
    let json = encode_text(Envelope::Json, "ch", "hi", None).unwrap();
    assert!(json.starts_with('{'));
    for raw in [&cbor, &json] {
        let read = decode(raw).unwrap();
        assert_eq!(read.channel, "ch");
        assert_eq!(read.body, Body::Text("hi".into()));
    }
}

#[test]
fn bytes_round_trip_in_both_envelopes_and_the_limit_is_on_the_final_text() {
    let blob: Vec<u8> = (0..=255).cycle().take(3000).collect();
    for envelope in [Envelope::Cbor, Envelope::Json] {
        let raw = encode_bytes(envelope, "c", &blob).unwrap();
        assert_eq!(decode(&raw).unwrap().body, Body::Bytes(blob.clone().into()));
        assert!(raw.len() <= NOTIFY_LIMIT);
        // Base64 grows the payload by a third: this fits as bytes only
        // below roughly three quarters of the limit.
        let over = encode_bytes(envelope, "c", &vec![0; NOTIFY_LIMIT]).unwrap_err();
        assert!(over > NOTIFY_LIMIT);
    }
}

/// What a 0.20.5 reader parses: no `b` field.
#[derive(Deserialize)]
struct Legacy {
    c: String,
    p: Option<String>,
    r: Option<uuid::Uuid>,
}

#[test]
fn the_json_envelope_is_what_a_0_20_5_reader_parses() {
    let raw = encode_text(Envelope::Json, "ch", "{\"a\":1}", None).unwrap();
    let old: Legacy = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        (old.c.as_str(), old.p.as_deref()),
        ("ch", Some("{\"a\":1}"))
    );
    // A byte payload parses too and carries nothing it knows: skipped.
    let raw = encode_bytes(Envelope::Json, "ch", b"\x00\xff").unwrap();
    let old: Legacy = serde_json::from_str(&raw).unwrap();
    assert!(old.p.is_none() && old.r.is_none());
}

#[test]
fn a_legacy_notification_as_0_20_5_wrote_it_is_read() {
    let id = "00000000-0000-0000-0000-000000000009";
    let inline = r#"{"c":"cratestack.model.W","p":"{\"x\":1}","n":4}"#;
    assert_eq!(text(inline), "{\"x\":1}");
    let by_ref = format!(r#"{{"c":"c","r":"{id}","n":5}}"#);
    assert_eq!(
        decode(&by_ref).unwrap().body,
        Body::Outbox(uuid::Uuid::from_u128(9))
    );
    // Before the nonce existed.
    assert_eq!(text(r#"{"c":"c","p":"hi"}"#), "hi");
}

#[test]
fn unreadable_notifications_say_why() {
    let cbor = |bytes: &[u8]| format!("{CBOR_PREFIX}{}", STANDARD.encode(bytes));
    // `{"v": 2, "c": "c", "t": "x"}`
    let v2 = cbor(&[
        0xa3, 0x61, b'v', 0x02, 0x61, b'c', 0x61, b'c', 0x61, b't', 0x61, b'x',
    ]);
    // `{"c": "c", "t": "x"}`: no version
    let unversioned = cbor(&[0xa2, 0x61, b'c', 0x61, b'c', 0x61, b't', 0x61, b'x']);
    // `{"v": 1, "c": "c"}`: no payload
    let empty = cbor(&[0xa2, 0x61, b'v', 0x01, 0x61, b'c', 0x61, b'c']);
    for (raw, why) in [
        (format!("{CBOR_PREFIX}!!not base64!!"), "base64"),
        (cbor(&[0xff, 0xff]), "CBOR"),
        (cbor(&[0xa1, 0x61]), "CBOR"),
        (v2, "version 2"),
        (unversioned, "version"),
        (empty, "no payload"),
        ("plain text".to_owned(), "envelope"),
        ("{not json".to_owned(), "key must be"),
        (r#"{"c":"c"}"#.to_owned(), "no payload"),
        (String::new(), "envelope"),
    ] {
        let reason = decode(&raw).unwrap_err();
        assert!(reason.contains(why), "{raw:?}: {reason}");
    }
}

#[test]
fn a_later_minor_version_may_add_keys() {
    // `{"v": 1, "c": "c", "x": [1, 2], "t": "x"}`
    let raw = format!(
        "{CBOR_PREFIX}{}",
        STANDARD.encode([
            0xa4, 0x61, b'v', 0x01, 0x61, b'c', 0x61, b'c', 0x61, b'x', 0x82, 0x01, 0x02, 0x61,
            b't', 0x61, b'x'
        ])
    );
    assert_eq!(text(&raw), "x");
}
