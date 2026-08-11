//! cratestack#407: settles, with a real generated-client integration test
//! rather than inference, the issue's open question — does the generated
//! `cratestack-client-rust` call site treat a declared `@status(202)`
//! response as success?
//!
//! `crates/cratestack-client-rust/src/client/transport.rs`'s
//! `if !status.is_success() { ... }` check (`http`/`reqwest`'s
//! `StatusCode::is_success`, true for the entire `200..300` range) is the
//! source-level evidence this was inferred from in the issue. This test
//! proves it end-to-end: a mock server that answers with a bare `202` (no
//! `200` anywhere in the exchange) still round-trips through the generated
//! `procedures().submit(...)` call site as `Ok(SubmitReply { .. })`, not an
//! error.
//!
//! Only the Rust client is verified here — see this PR's description for
//! why the Dart client's equivalent behavior (Dio's default
//! `validateStatus`, `200 <= status < 300`) was NOT independently run and
//! is not asserted as verified.

mod support;

mod schema {
    cratestack::include_client_schema!("tests/fixtures/status_override.cstack");
}

use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};

#[tokio::test]
async fn client_treats_declared_status_202_as_success_not_error() {
    let (base_url, _server) = support::spawn_mock_server(|request| {
        assert_eq!(request.path, "/$procs/submit");
        assert_eq!(request.method, "POST");
        // Deliberately a bare 202 — no 200 anywhere in this exchange — so a
        // client-generator bug that hardcoded "success == exactly 200"
        // would surface here as a `Remote` error instead of `Ok(..)`.
        support::cbor_status(
            202,
            &schema::cratestack_schema::SubmitReply {
                echo: "hello".to_owned(),
            },
        )
    })
    .await;

    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = schema::cratestack_schema::client::Client::new(runtime);

    let reply = client
        .procedures()
        .submit(
            &schema::cratestack_schema::procedures::submit::Args {
                args: schema::cratestack_schema::SubmitArgs {
                    message: "hello".to_owned(),
                },
            },
            &[],
        )
        .await
        .expect("a declared @status(202) response should decode as success, not an error");

    assert_eq!(reply.echo, "hello");
}

/// Control case: an out-of-band non-2xx status (`500`, which no procedure
/// in this fixture ever declares via `@status`) still surfaces as an
/// error — proving the assertion above is actually exercising
/// success-path decoding, not a client that accepts every status
/// unconditionally.
#[tokio::test]
async fn client_still_treats_5xx_as_an_error() {
    let (base_url, _server) = support::spawn_mock_server(|_request| {
        support::cbor_status(
            500,
            &schema::cratestack_schema::SubmitReply {
                echo: "hello".to_owned(),
            },
        )
    })
    .await;

    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = schema::cratestack_schema::client::Client::new(runtime);

    let error = client
        .procedures()
        .submit(
            &schema::cratestack_schema::procedures::submit::Args {
                args: schema::cratestack_schema::SubmitArgs {
                    message: "hello".to_owned(),
                },
            },
            &[],
        )
        .await
        .expect_err("a 500 response should still surface as an error");
    let _ = error;
}
