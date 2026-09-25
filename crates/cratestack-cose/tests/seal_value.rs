//! `seal_value` encodes the payload in place (ADR 0006 §1; maintainer
//! decision on cratestack#1005) and produces exactly the bytes that
//! `encode` followed by `seal` produces.

mod common;
#[path = "common/in_place.rs"]
mod in_place;

use bytes::Bytes;
use common::fixture::{payment, payment_bytes};
use common::forge::layout;
use common::{CTI_2, CTI_16, IAT, hex, rest_request, rpc_request};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::rpc::RpcErrorBody;
use cratestack_core::{Binding, CratestackCodec, CratestackEnvelope};
use cratestack_cose::CoseAlg;
use in_place::{RecordingCbor, Text};

/// Through the trait, the way a router calls it.
async fn via_seal_value<E: CratestackEnvelope, T: serde::Serialize + Sync>(
    envelope: &E,
    value: &T,
    bind: &Binding<'_>,
) -> Bytes {
    envelope
        .seal_value(&CborCodec, value, bind)
        .await
        .expect("seal_value")
}

async fn via_encode_then_seal<E: CratestackEnvelope, T: serde::Serialize>(
    envelope: &E,
    value: &T,
    bind: &Binding<'_>,
) -> Bytes {
    let payload = CborCodec.encode(value).expect("encode");
    envelope
        .seal(Bytes::from(payload), bind)
        .await
        .expect("seal")
}

/// Every positive vector, sealed the in-place way, is the checked-in byte
/// string: requests (both `cti` shapes), 200 responses, error responses,
/// responses to unsigned GETs. `unary.json` is itself produced through
/// `seal`, so this is `seal_value` == `encode` + `seal`, for every vector.
#[tokio::test]
async fn seal_value_reproduces_every_vector() {
    let file: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/vectors/unary.json"
        ))
        .expect("vectors"),
    )
    .expect("json");
    let error = RpcErrorBody {
        code: "not_found".to_owned(),
        message: "payment not found".to_owned(),
        details: None,
    };
    let mut checked = 0;
    for case in file["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let alg = CoseAlg::from_id(case["alg_id"].as_i64().expect("alg")).expect("known alg");
        let bind = common::binding_from_json(&case["binding"]);
        let payload = case["payload"].as_str().expect("payload");
        let sealed = if case["direction"] == "request" {
            let cti = case["cti"].as_str().expect("cti");
            let client = common::client(alg, IAT, cti);
            if payload == hex(&payment_bytes()) {
                via_seal_value(&client, &payment(), &bind).await
            } else {
                panic!("{name}: unexpected request payload")
            }
        } else {
            let server = common::server(alg, IAT);
            if payload == hex(&payment_bytes()) {
                via_seal_value(&server, &payment(), &bind).await
            } else {
                assert_eq!(
                    payload,
                    hex(&CborCodec.encode(&error).expect("error")),
                    "{name}"
                );
                via_seal_value(&server, &error, &bind).await
            }
        };
        assert_eq!(hex(&sealed), case["cose"].as_str().expect("cose"), "{name}");
        checked += 1;
    }
    assert!(checked >= 33, "only {checked} vectors");
}

/// Payload sizes on both sides of every `bstr` head-length boundary (1, 2,
/// 3 and 5 head bytes), for every algorithm, as request and as response.
#[tokio::test]
async fn seal_value_equals_encode_then_seal_at_every_head_size() {
    // `Text::of(n)` encodes to n + head(n) bytes: 23, 24, 255, 256, 65535,
    // 65536 bytes of payload and a little around them.
    let lens = [
        0, 22, 23, 24, 252, 253, 254, 255, 65_531, 65_532, 65_533, 70_000,
    ];
    for &alg in CoseAlg::ALL {
        for cti in [CTI_16, CTI_2] {
            let client = common::client(alg, IAT, cti);
            let server = common::server(alg, IAT);
            let response = common::response_to(&rest_request(), b"request", 200);
            for len in lens {
                let value = Text::of(len);
                assert_eq!(
                    via_seal_value(&client, &value, &rpc_request()).await,
                    via_encode_then_seal(&client, &value, &rpc_request()).await,
                    "{alg:?} request, text {len}"
                );
                let in_place = via_seal_value(&server, &value, &response).await;
                assert_eq!(
                    in_place,
                    via_encode_then_seal(&server, &value, &response).await,
                    "{alg:?} response, text {len}"
                );
                client
                    .open_response(in_place, &response)
                    .await
                    .expect("the in-place response opens");
            }
        }
    }
}

/// The payload bytes in the sealed message are the bytes the codec wrote,
/// at the address it wrote them: `encode_into` ran on the message buffer
/// itself, and nothing moved or copied the payload afterwards (the prefix
/// and the head were written around it, and the leading slack is skipped
/// by moving a pointer).
///
/// The codec reserves room for the payload **and the signature that
/// follows it** first. Without that, the one buffer may have to grow (while
/// the codec writes, or to append the signature), and growing a `Vec` is a
/// reallocation the allocator may satisfy by moving it, exactly as it
/// would the codec's own `Vec`. That is buffer growth, not the second
/// buffer and the copy between the two that `seal_value` removes, and
/// whether it moves depends on the allocator; reserving takes it out of
/// the measurement. (With `payload + 64` reserved, a 70 000-byte ESP256
/// message moved on the signature's append: 66 bytes did not fit.)
#[tokio::test]
async fn the_payload_is_encoded_where_it_is_sent() {
    for &alg in CoseAlg::ALL {
        for len in [10, 300, 70_000] {
            let codec = RecordingCbor {
                // payload head (at most 5) + the payload + the signature's
                // bstr (at most 66), with room to spare.
                reserve: len + 256,
                ..RecordingCbor::default()
            };
            let value = Text::of(len);
            let envelope = common::client(alg, IAT, CTI_16);
            let sealed = envelope
                .seal_value(&codec, &value, &rpc_request())
                .await
                .expect("seal_value");
            let (wrote_at, wrote_len) = codec.wrote.lock().expect("lock").expect("encode_into ran");
            let payload = layout_any(&sealed);
            assert_eq!(wrote_len, payload.len(), "{alg:?} {len}");
            assert_eq!(
                sealed[payload.clone()].as_ptr() as usize,
                wrote_at,
                "{alg:?} {len}: the payload was moved or copied after encoding"
            );
            // And it opens.
            common::server(alg, IAT)
                .open_request(sealed.clone(), &rpc_request())
                .await
                .expect("opens");
        }
    }
}

/// `forge::layout` reads 1- and 2-byte heads only; a 70 000-byte payload
/// has a 5-byte head.
fn layout_any(bytes: &[u8]) -> std::ops::Range<usize> {
    let head = |at: usize| -> (usize, usize) {
        match bytes[at] {
            h @ 0x40..=0x57 => (1, usize::from(h - 0x40)),
            0x58 => (2, usize::from(bytes[at + 1])),
            0x59 => (
                3,
                usize::from(u16::from_be_bytes([bytes[at + 1], bytes[at + 2]])),
            ),
            0x5a => (
                5,
                usize::try_from(u32::from_be_bytes(
                    bytes[at + 1..at + 5].try_into().expect("4"),
                ))
                .expect("fits"),
            ),
            other => panic!("head {other:#x}"),
        }
    };
    let (h, protected_len) = head(2);
    let payload_head = 2 + h + protected_len + 1;
    let (h, len) = head(payload_head);
    let range = payload_head + h..payload_head + h + len;
    if len < 256 {
        assert_eq!(range, layout(bytes).payload);
    }
    range
}

/// The inherent typed methods take the same path as the trait method.
#[tokio::test]
async fn the_inherent_value_methods_match_too() {
    for &alg in CoseAlg::ALL {
        let client = common::client(alg, IAT, CTI_16);
        assert_eq!(
            client
                .seal_request_value(&CborCodec, &payment(), &rpc_request())
                .await
                .expect("seal"),
            client
                .seal_request(&payment_bytes(), &rpc_request())
                .await
                .expect("seal")
        );
        let server = common::server(alg, IAT);
        let response = common::response_to(&rpc_request(), b"request", 201);
        assert_eq!(
            server
                .seal_response_value(&CborCodec, &payment(), &response)
                .await
                .expect("seal"),
            server
                .seal_response(&payment_bytes(), &response)
                .await
                .expect("seal")
        );
    }
}

/// A codec error comes back as the codec reported it, not as a signing
/// failure, and nothing is signed.
#[tokio::test]
async fn a_codec_error_is_the_codecs() {
    struct Unserializable;
    impl serde::Serialize for Unserializable {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("no"))
        }
    }
    let client = common::client(CoseAlg::Ed25519, IAT, CTI_16);
    let result = client
        .seal_value(&CborCodec, &Unserializable, &rpc_request())
        .await;
    assert!(matches!(
        result,
        Err(cratestack_core::CratestackError::Codec(_))
    ));
}
