//! Proof (issue #493) that the *generated* per-model REST client — not
//! just the underlying `CratestackClient` runtime — can do the
//! `@version` optimistic-locking round trip: `get_with_response` →
//! read `ETag` → `update_with_response` with `If-Match`.
//!
//! `versioned.cstack` is borrowed verbatim from
//! `crates/cratestack-pg/tests/fixtures/banking_versioning.cstack`, same
//! spirit as this crate's other fixtures (see `generated_client.rs`'s
//! module doc) — the mock server here plays the part of a real
//! CrateStack server's `@version` ETag/If-Match handling (proven
//! end-to-end against Postgres in `cratestack-pg`'s
//! `banking_versioning.rs`), so this test is scoped to proving the
//! generated client wires the new methods through correctly, not
//! re-proving server-side locking.

mod support;

mod versioned_schema {
    cratestack::include_client_schema!("tests/fixtures/versioned.cstack");
}

use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};
use cratestack_core::CoolCodec;
use versioned_schema::cratestack_schema::{Ledger, UpdateLedgerInput};

#[tokio::test]
async fn generated_client_reaches_etag_then_if_match_round_trip() {
    let (base_url, _server) = support::spawn_mock_server(|request| {
        if request.path == "/ledgers/4" && request.method == "GET" {
            return support::cbor_ok_with_headers(
                &Ledger {
                    id: 4,
                    label: "gl-4".to_owned(),
                    balance: 1,
                    version: 0,
                },
                vec![("etag".to_owned(), "\"0\"".to_owned())],
            );
        }
        if request.path == "/ledgers/4" && request.method == "PATCH" {
            let if_match = request.headers.get("if-match").map(String::as_str);
            if if_match != Some("\"0\"") {
                return support::MockResponse {
                    status: 412,
                    content_type: CborCodec::CONTENT_TYPE.to_owned(),
                    body: Vec::new(),
                    extra_headers: Vec::new(),
                };
            }
            return support::cbor_ok_with_headers(
                &Ledger {
                    id: 4,
                    label: "gl-4".to_owned(),
                    balance: 5,
                    version: 1,
                },
                vec![("etag".to_owned(), "\"1\"".to_owned())],
            );
        }
        support::not_found()
    })
    .await;

    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = versioned_schema::cratestack_schema::client::Client::new(runtime);
    let ledgers = client.ledgers();

    // GET through the generated client — read the ETag the plain `get`
    // would have thrown away.
    let get_response = ledgers
        .get_with_response(&4, &[])
        .await
        .expect("generated get_with_response should succeed");
    assert_eq!(get_response.value.version, 0);
    let etag = get_response
        .header("etag")
        .expect("etag must survive decoding through the generated client")
        .to_owned();
    assert_eq!(etag, "\"0\"");

    // PATCH through the generated client, sending the learned ETag as
    // If-Match — this is the exact round trip #493 says the typed
    // client could not previously express.
    let update_response = ledgers
        .update_with_response(
            &4,
            &UpdateLedgerInput {
                label: None,
                balance: Some(5),
            },
            &[("if-match", etag.as_str())],
        )
        .await
        .expect("generated update_with_response with a fresh If-Match should succeed");
    assert_eq!(update_response.value.balance, 5);
    assert_eq!(update_response.value.version, 1);
    assert_eq!(update_response.header("etag"), Some("\"1\""));
}
