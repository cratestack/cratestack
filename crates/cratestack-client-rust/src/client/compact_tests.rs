//! Unit tests for the compact byte work that need no server.

use cratestack_codec_json::compact::json_to_compact;
use cratestack_core::compact::{COMPACT_JSON, KeyNode, KeyStruct, KeyTables, OpKeys};
use reqwest::Method;

use crate::client::route::RouteRef;
use crate::config::ClientConfig;
use crate::runtime::wire::{RuntimeHeader, RuntimeResponseWire};
use crate::{CborCodec, CratestackClient};

fn tables() -> KeyTables {
    KeyTables {
        types: vec![KeyStruct {
            fields: vec![("id".to_owned(), KeyNode::Opaque)],
        }],
        ops: vec![OpKeys {
            op: "GET /widgets/{id}".to_owned(),
            digest: [3; 32],
            request: KeyNode::Opaque,
            response: KeyNode::Struct(0),
        }],
    }
}

fn call() -> super::compact::CompactCall {
    let client = CratestackClient::new(
        ClientConfig::new("http://example.invalid".parse().expect("url")),
        CborCodec,
    )
    .with_compact_keys(tables());
    client
        .at(RouteRef::new("/widgets/{id}", &["1"]))
        .compact_call(&Method::GET, &[], false)
        .expect("a REST op is found by `METHOD template`")
}

fn response(content_type: &str, body: &[u8]) -> RuntimeResponseWire {
    RuntimeResponseWire {
        status_code: 200,
        headers: vec![RuntimeHeader {
            name: "Content-Type".to_owned(),
            value: content_type.to_owned(),
        }],
        body: body.to_vec(),
    }
}

#[test]
fn the_selector_header_name_is_the_contract_header() {
    assert!(
        cratestack_core::CONTRACT_HEADER.eq_ignore_ascii_case("cratestack-contract"),
        "stamp_selector spells the header literally"
    );
}

#[test]
fn a_compact_json_response_becomes_named_json_and_turns_requests_compact() {
    let call = call();
    assert!(!call.state.is_compact(&call.op));
    let compact =
        json_to_compact(&call.state.tables, &KeyNode::Struct(0), br#"{"id":5}"#).expect("compact");
    assert_eq!(compact, br#"{"0":5}"#);
    let opened = call
        .open_response(response(COMPACT_JSON, &compact))
        .expect("opens");
    assert_eq!(opened.body, br#"{"id":5}"#);
    assert_eq!(opened.headers[0].value, "application/json");
    assert!(call.state.is_compact(&call.op));
}

#[test]
fn a_415_or_a_plain_answer_turns_requests_named_again() {
    let call = call();
    call.state.set_compact(&call.op, true);
    call.note_failure(415);
    assert!(!call.state.is_compact(&call.op));
    call.state.set_compact(&call.op, true);
    call.open_response(response("application/json", b"{}"))
        .expect("plain passes");
    assert!(!call.state.is_compact(&call.op));
}

#[test]
fn the_stream_error_item_passes_through_untouched() {
    let call = call();
    // Tag(48900, {"id": 1}): a struct node would otherwise rewrite the key.
    let item = [0xd9, 0xbf, 0x04, 0xa1, 0x62, b'i', b'd', 0x01];
    assert_eq!(call.item_transcoder().item(&item).expect("kept"), item);
}
