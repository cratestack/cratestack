//! ADR 0019 PR B (B4): the generated Rust client carries a `BigInt` as the
//! canonical decimal string, on REST and on RPC, against a mock server whose
//! request bytes the test reads.
//!
//! What this pins that the macros' token tests cannot:
//! - the primary key is a path segment (`/ledgers/9223372036854775807`) on
//!   REST and a text string in `RpcPkInput` on RPC, never a number;
//! - request bodies (update input, procedure arguments) write every `BigInt`
//!   as a CBOR text string, including the value above 2^53 a JavaScript
//!   number would round;
//! - a response that sends a CBOR integer where a `BigInt` belongs is a
//!   client error, not a silently truncated value;
//! - `<Model>Where` accepts a `BigInt` filter and produces a filter for it.

mod support;

mod rest_schema {
    cratestack::include_client_schema!("tests/fixtures/bigint_client_rest.cstack");
}

mod rpc_schema {
    cratestack::include_client_schema!("tests/fixtures/bigint_client_rpc.cstack");
}

use std::sync::{Arc, Mutex};

use cratestack::BigInt;
use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};
use cratestack_core::CratestackCodec;
use serde_json::{Value as Json, json};

const I64_MAX: i64 = i64::MAX;
const ABOVE_2_53: i64 = 9_007_199_254_740_993;

/// The CBOR of the text `9223372036854775807`: major type 3, length 19.
fn max_as_cbor_text() -> Vec<u8> {
    let mut bytes = vec![0x73];
    bytes.extend_from_slice(b"9223372036854775807");
    bytes
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// A captured request body, decoded the way the server would: CBOR into a
/// `serde_json::Value`, where a text string stays a string and an integer
/// stays a number, so the assertion can tell them apart.
fn decode_body(captured: &Mutex<Option<Vec<u8>>>) -> Json {
    let bytes = captured
        .lock()
        .unwrap()
        .clone()
        .expect("the mock server saw a request body");
    CborCodec.decode(&bytes).expect("request body is CBOR")
}

macro_rules! ledger {
    ($schema:ident) => {
        $schema::cratestack_schema::Ledger {
            id: BigInt::new(I64_MAX),
            label: "gl".to_owned(),
            balance: BigInt::new(ABOVE_2_53),
            fee: Some(BigInt::new(i64::MIN)),
            revision: BigInt::new(ABOVE_2_53),
        }
    };
}

#[test]
fn a_bigint_model_encodes_every_field_as_cbor_text() {
    let bytes = CborCodec.encode(&ledger!(rest_schema)).unwrap();
    assert!(
        contains(&bytes, &max_as_cbor_text()),
        "i64::MAX must be 0x73 + 19 ASCII digits, not an integer: {bytes:02x?}"
    );
    // 1b7fffffffffffffff is the integer form `Int` would take.
    assert!(!contains(
        &bytes,
        &[0x1b, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
    ));
}

#[tokio::test]
async fn rest_get_puts_the_bigint_key_in_the_path_and_decodes_the_row() {
    let (base_url, _server) = support::spawn_mock_server(|request| {
        if request.path == "/ledgers/9223372036854775807" && request.method == "GET" {
            return support::cbor_ok_with_headers(
                &ledger!(rest_schema),
                vec![("etag".to_owned(), format!("\"{ABOVE_2_53}\""))],
            );
        }
        support::not_found()
    })
    .await;
    let client = rest_schema::cratestack_schema::client::Client::new(CratestackClient::new(
        ClientConfig::new(base_url),
        CborCodec,
    ));

    let response = client
        .ledgers()
        .get_with_response(&BigInt::new(I64_MAX), &[])
        .await
        .expect("the key is the canonical decimal in the path");
    assert_eq!(response.value, ledger!(rest_schema));
    assert_eq!(response.value.id.get(), I64_MAX);
    assert_eq!(response.value.fee, Some(BigInt::MIN));
    // The `@version` column is a BigInt and the ETag header stays a number.
    assert_eq!(response.header("etag"), Some("\"9007199254740993\""));
}

#[tokio::test]
async fn rest_update_sends_a_bigint_as_text_with_if_match() {
    let captured = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&captured);
    let (base_url, _server) = support::spawn_mock_server(move |request| {
        if request.path == "/ledgers/9223372036854775807" && request.method == "PATCH" {
            *seen.lock().unwrap() = Some(request.body.clone());
            return support::cbor_ok(&ledger!(rest_schema));
        }
        support::not_found()
    })
    .await;
    let client = rest_schema::cratestack_schema::client::Client::new(CratestackClient::new(
        ClientConfig::new(base_url),
        CborCodec,
    ));

    client
        .ledgers()
        .update(
            &BigInt::new(I64_MAX),
            &rest_schema::cratestack_schema::UpdateLedgerInput {
                label: None,
                balance: Some(BigInt::new(ABOVE_2_53)),
                fee: Some(Some(BigInt::new(-1))),
            },
            &[("if-match", "\"9007199254740993\"")],
        )
        .await
        .expect("update");
    let body = decode_body(&captured);
    assert_eq!(body["balance"], json!("9007199254740993"), "{body}");
    assert_eq!(body["fee"], json!("-1"), "{body}");
}

#[tokio::test]
async fn rest_procedure_argument_and_result_are_strings() {
    let captured = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&captured);
    let (base_url, _server) = support::spawn_mock_server(move |request| {
        if request.path.starts_with("/$procs/lookup") && request.method == "POST" {
            *seen.lock().unwrap() = Some(request.body.clone());
            return support::cbor_ok(&rest_schema::cratestack_schema::Quote {
                amount: BigInt::new(I64_MAX),
                maybe: None,
                many: vec![BigInt::new(ABOVE_2_53), BigInt::MIN],
            });
        }
        support::not_found()
    })
    .await;
    let client = rest_schema::cratestack_schema::client::Client::new(CratestackClient::new(
        ClientConfig::new(base_url),
        CborCodec,
    ));

    let quote = client
        .procedures()
        .lookup(
            &rest_schema::cratestack_schema::procedures::lookup::Args {
                owner: BigInt::new(ABOVE_2_53),
            },
            &[],
        )
        .await
        .expect("lookup");
    assert_eq!(quote.amount, BigInt::MAX);
    assert_eq!(quote.maybe, None);
    assert_eq!(quote.many, vec![BigInt::new(ABOVE_2_53), BigInt::MIN]);
    assert_eq!(
        decode_body(&captured),
        json!({ "owner": "9007199254740993" })
    );
}

#[tokio::test]
async fn a_cbor_integer_where_a_bigint_belongs_is_a_client_error() {
    let (base_url, _server) = support::spawn_mock_server(|request| {
        if request.path == "/ledgers/5" && request.method == "GET" {
            // `id` as a CBOR integer: what a pre-BigInt server would send.
            return support::cbor_ok(&json!({
                "id": 5, "label": "gl", "balance": "1", "fee": null, "revision": "0"
            }));
        }
        support::not_found()
    })
    .await;
    let client = rest_schema::cratestack_schema::client::Client::new(CratestackClient::new(
        ClientConfig::new(base_url),
        CborCodec,
    ));

    let result = client.ledgers().get(&BigInt::new(5), &[]).await;
    assert!(
        result.is_err(),
        "a number must not decode as a BigInt: {result:?}"
    );
}

#[test]
fn where_filters_a_bigint_field_and_the_filter_survives() {
    use rest_schema::cratestack_schema::LedgerWhere;
    let filter = LedgerWhere {
        balance: Some(cratestack::FieldFilterInput {
            gt: Some(BigInt::new(ABOVE_2_53)),
            in_: Some(vec![BigInt::new(1), BigInt::new(2)]),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        filter.to_filters().len(),
        2,
        "gt and in_ both reach the wire"
    );

    // On the wire the operands are strings too.
    let wire = serde_json::to_value(&filter).unwrap();
    assert_eq!(wire["balance"]["gt"], json!("9007199254740993"));
    assert_eq!(wire["balance"]["in"], json!(["1", "2"]));
    // A JSON number is not a BigInt operand.
    assert!(serde_json::from_value::<LedgerWhere>(json!({"balance": {"gt": 5}})).is_err());
}

#[tokio::test]
async fn rpc_get_and_delete_send_the_key_as_a_text_string() {
    let captured = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&captured);
    let (base_url, _server) = support::spawn_mock_server(move |request| {
        if request.method == "POST"
            && (request.path == "/rpc/model.Ledger.get"
                || request.path == "/rpc/model.Ledger.delete")
        {
            *seen.lock().unwrap() = Some(request.body.clone());
            return support::cbor_ok(&ledger!(rpc_schema));
        }
        support::not_found()
    })
    .await;
    let client = rpc_schema::cratestack_schema::client::Client::new(CratestackClient::new(
        ClientConfig::new(base_url),
        CborCodec,
    ));

    let row = client
        .ledgers()
        .get(&BigInt::new(I64_MAX))
        .await
        .expect("get");
    assert_eq!(row, ledger!(rpc_schema));
    let body = decode_body(&captured);
    assert_eq!(body["id"], json!("9223372036854775807"), "{body}");

    client
        .ledgers()
        .delete(&BigInt::new(I64_MAX))
        .await
        .expect("delete");
    assert_eq!(decode_body(&captured)["id"], json!("9223372036854775807"));
}

#[tokio::test]
async fn rpc_procedure_argument_is_a_text_string() {
    let captured = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&captured);
    let (base_url, _server) = support::spawn_mock_server(move |request| {
        if request.path == "/rpc/procedure.lookup" && request.method == "POST" {
            *seen.lock().unwrap() = Some(request.body.clone());
            return support::cbor_ok(&rpc_schema::cratestack_schema::Quote {
                amount: BigInt::MIN,
                maybe: Some(BigInt::new(0)),
                many: Vec::new(),
            });
        }
        support::not_found()
    })
    .await;
    let client = rpc_schema::cratestack_schema::client::Client::new(CratestackClient::new(
        ClientConfig::new(base_url),
        CborCodec,
    ));

    let quote = client
        .procedures()
        .lookup(&rpc_schema::cratestack_schema::procedures::lookup::Args {
            owner: BigInt::new(ABOVE_2_53),
        })
        .await
        .expect("lookup");
    assert_eq!(quote.amount, BigInt::MIN);
    assert_eq!(quote.maybe, Some(BigInt::new(0)));
    assert_eq!(
        decode_body(&captured),
        json!({ "owner": "9007199254740993" })
    );
}
