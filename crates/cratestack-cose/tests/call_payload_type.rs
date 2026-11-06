//! `CallBinding` payload types (compact wire keys, §5.1 "Sealed (COSE)"):
//! a Dart or wasm app seals under `application/vnd.cratestack.compact+cbor`
//! and reads answers under the types it accepted, the way the Rust client
//! does (cratestack#1168).

mod common;

use bytes::Bytes;
use common::{AUDIENCE, CTI_16, IAT, client, contract_sha, server};
use cratestack_core::{CratestackError, PAYLOAD_ACCEPT_HEADER, PAYLOAD_TYPE_HEADER};
use cratestack_cose::{CallBinding, CoseAlg};

const COMPACT: &str = "application/vnd.cratestack.compact+cbor";
const CBOR: &str = "application/cbor";
const ALG: CoseAlg = CoseAlg::Hmac256_64;

fn call(payload_type: Option<&str>, payload_accept: Option<&[&str]>) -> CallBinding {
    CallBinding {
        audience: AUDIENCE.to_owned(),
        method: "POST".to_owned(),
        route: "model.Payment.create".to_owned(),
        path_params: Vec::new(),
        query: None,
        contract_sha: contract_sha(),
        idempotency_key: None,
        if_match: None,
        payload_type: payload_type.map(str::to_owned),
        payload_accept: payload_accept
            .map(|types| types.iter().map(|entry| (*entry).to_owned()).collect()),
    }
}

#[test]
fn a_default_call_is_cbor_both_ways_and_sends_no_selector_header() {
    let call = call(None, None);
    call.validate().expect("valid");
    assert_eq!(call.request().payload_media_type, CBOR);
    assert!(call.selector_headers().is_empty());
    // Spelling the defaults out changes nothing.
    let explicit = self::call(Some(CBOR), Some(&[CBOR]));
    assert_eq!(explicit.request(), call.request());
    assert!(explicit.selector_headers().is_empty());
}

#[test]
fn a_compact_call_names_its_types_in_the_headers_and_the_aad() {
    let call = call(Some(COMPACT), Some(&[COMPACT, CBOR]));
    call.validate().expect("valid");
    assert_eq!(call.request().payload_media_type, COMPACT);
    assert_eq!(
        call.selector_headers(),
        [
            (PAYLOAD_TYPE_HEADER, COMPACT.to_owned()),
            (PAYLOAD_ACCEPT_HEADER, format!("{COMPACT}, {CBOR}")),
        ]
    );
    assert_ne!(
        cratestack_cose::external_aad(&call.request()).unwrap(),
        cratestack_cose::external_aad(&self::call(None, None).request()).unwrap()
    );
}

#[tokio::test]
async fn a_request_sealed_under_the_compact_type_opens_only_under_it() {
    let compact = call(Some(COMPACT), None);
    let sealed = client(ALG, IAT, CTI_16)
        .seal_request(b"\xa1\x01\x02", &compact.request())
        .await
        .expect("seal");
    let opened = server(ALG, IAT)
        .open_request(sealed.clone(), &compact.request())
        .await
        .expect("the server opens it under the compact type");
    assert_eq!(opened.payload.as_ref(), b"\xa1\x01\x02");
    let wrong = server(ALG, IAT)
        .open_request(sealed, &call(None, None).request())
        .await;
    assert!(matches!(wrong, Err(CratestackError::Unauthorized(_))));
}

#[tokio::test]
async fn a_response_under_any_accepted_type_opens_and_reports_it() {
    let call = call(Some(COMPACT), Some(&[COMPACT, CBOR]));
    let sealed = b"the sealed request";
    for named in [Some(COMPACT), Some(CBOR), None] {
        let server_side = call
            .response_for(sealed, 200, named)
            .expect("an accepted type");
        let answer = server(ALG, IAT)
            .seal_response(b"\xa0", &server_side)
            .await
            .expect("seal response");
        let client_side = call.response_for(sealed, 200, named).unwrap();
        assert_eq!(
            client_side.payload_media_type,
            named.unwrap_or(CBOR),
            "the reported type"
        );
        let opened = client(ALG, IAT, CTI_16)
            .open_response(Bytes::from(answer.to_vec()), &client_side)
            .await
            .expect("opens");
        assert_eq!(opened.payload.as_ref(), b"\xa0");
    }
}

#[tokio::test]
async fn a_response_sealed_under_another_type_than_its_header_says_is_refused() {
    let call = call(Some(COMPACT), Some(&[COMPACT, CBOR]));
    let sealed = b"the sealed request";
    let answer = server(ALG, IAT)
        .seal_response(
            b"\xa0",
            &call.response_for(sealed, 200, Some(COMPACT)).unwrap(),
        )
        .await
        .unwrap();
    let lying = call.response_for(sealed, 200, Some(CBOR)).unwrap();
    let result = client(ALG, IAT, CTI_16)
        .open_response(Bytes::from(answer.to_vec()), &lying)
        .await;
    assert!(matches!(result, Err(CratestackError::Unauthorized(_))));
}

#[test]
fn a_response_type_the_call_did_not_ask_for_is_refused() {
    let call = call(None, Some(&[COMPACT]));
    for named in [
        None,
        Some("application/json"),
        Some("APPLICATION/X"),
        Some(""),
    ] {
        assert!(
            matches!(
                call.response_for(b"x", 200, named),
                Err(CratestackError::Unauthorized(_))
            ),
            "{named:?}"
        );
    }
    assert!(call.response_for(b"x", 200, Some(COMPACT)).is_ok());
}

#[test]
fn types_that_cannot_be_sealed_are_refused_before_sealing() {
    let bad_types = [
        "application/vnd.cratestack.compact+cbor-seq",
        "application/cbor-seq",
        "text/event-stream",
        "application/cose",
        "multipart/form-data",
        "Application/Cbor",
        "application/json; charset=utf-8",
        "",
    ];
    for bad in bad_types {
        assert!(call(Some(bad), None).validate().is_err(), "payload {bad:?}");
        assert!(
            call(None, Some(&[CBOR, bad])).validate().is_err(),
            "accept {bad:?}"
        );
    }
    assert!(call(None, Some(&[])).validate().is_err(), "empty accept");
    assert!(
        call(None, Some(&[CBOR, CBOR])).validate().is_err(),
        "duplicate accept"
    );
}
