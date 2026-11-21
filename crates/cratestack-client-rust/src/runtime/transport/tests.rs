//! The bridge's response conversion reads a CBOR byte string (cratestack#1425).

use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;
use reqwest::Url;

use super::RuntimeTransportClient;
use crate::client::CratestackClient;
use crate::config::ClientConfig;
use crate::runtime::wire::RuntimeResponseWire;

fn bridge(body: Vec<u8>) -> serde_json::Value {
    let client = RuntimeTransportClient::Cbor(CratestackClient::new(
        ClientConfig::new(Url::parse("http://localhost").expect("url")),
        CborCodec,
    ));
    let response = RuntimeResponseWire {
        status_code: 200,
        headers: Vec::new(),
        body,
    };
    let converted = client
        .transport_response_to_bridge(response)
        .expect("the bridge reads the body");
    serde_json::from_slice(&converted.body).expect("bridge body is JSON")
}

#[test]
fn a_cbor_byte_string_reads_as_the_integer_array_a_json_body_carries() {
    // {"data": h'0102ff'}
    let body = vec![0xa1, 0x64, b'd', b'a', b't', b'a', 0x43, 0x01, 0x02, 0xff];
    assert_eq!(bridge(body), serde_json::json!({"data": [1, 2, 255]}));
}

#[test]
fn the_pre_1425_array_form_still_reads() {
    let body = CborCodec
        .encode(&serde_json::json!({"data": [1, 2, 255]}))
        .expect("encode");
    assert_eq!(bridge(body), serde_json::json!({"data": [1, 2, 255]}));
}
