//! cratestack#1036 (ADR 0002 Q4/D3): `include_client_schema!` accepts a
//! schema that declares an MCP surface, and ignores it.
//!
//! The same schema is a `compile_error!` under `include_server_schema!` (until
//! the MCP runtime ships, cratestack#1033) and under
//! `include_embedded_schema!` (permanently) — see `cratestack-macros`'
//! `tests/ui_mcp.rs`. A client is different: it treats another service's
//! schema as a contract, and that service's MCP exposure is not the client's
//! concern. So the proof here is twofold: this file compiling at all is the
//! "accepted" half, and the round trips below show the generated client is
//! the ordinary REST client — the `@mcp`/`@@mcp` declarations add nothing to
//! and take nothing from the paths it calls.

mod support;

mod schema {
    cratestack::include_client_schema!("tests/fixtures/mcp.cstack");
}

use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};

use schema::cratestack_schema::{FeedArgs, Post, client::Client, procedures::get_feed};

#[tokio::test]
async fn mcp_declarations_compile_and_leave_the_client_unchanged() {
    let (base_url, _server) = support::spawn_mock_server(|request| {
        let post = Post {
            id: 1,
            title: "Hello".to_owned(),
        };
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/posts") => support::cbor_ok(&vec![post]),
            ("POST", "/$procs/getFeed") => support::cbor_ok(&vec![post]),
            _ => support::not_found(),
        }
    })
    .await;

    let client = Client::new(CratestackClient::new(
        ClientConfig::new(base_url),
        CborCodec,
    ));

    // The `@@mcp(resource: "posts")` model: still the plain REST model route,
    // not anything derived from the MCP segment.
    let posts = client
        .posts()
        .list(&[], &[])
        .await
        .expect("model list should succeed");
    assert_eq!(posts[0].title, "Hello");

    // The `@mcp(tool)` procedure: still `/$procs/<procedure name>`.
    let feed = client
        .procedures()
        .get_feed(
            &get_feed::Args {
                args: FeedArgs { limit: 1 },
            },
            &[],
        )
        .await
        .expect("procedure call should succeed");
    assert_eq!(feed[0].id, 1);
}
