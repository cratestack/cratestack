//! Empirical proof (cratestack#490) that `include_client_schema!` compiles
//! and works against this facade alone — no `cratestack-pg`/`cratestack-api`
//! in this crate's own graph, and (deliberately) no `axum` either, not even
//! as a dev-dependency, so this test suite's own mock server is a bare
//! `tokio::net::TcpListener` HTTP/1.1 parser rather than a real router. Two
//! schemas are exercised, both borrowed verbatim from
//! `crates/cratestack-pg/tests/fixtures/` so this is testing the same
//! surface those tests already cover, just compiled against the
//! `cratestack-axum`-free facade instead of `cratestack-pg`:
//!
//! - `blog.cstack` (REST transport, the default) — models, relations,
//!   projections/`select()`, paged models, CRUD inputs, and procedures
//!   (including a `Page<T>`-returning one).
//! - `transport_rpc.cstack` (`transport rpc`) — RPC model CRUD (`RpcClient`
//!   envelopes), a paged RPC model, and a sequence-returning procedure
//!   (`RpcStream`/`call_streaming`).

mod support;

mod rest_schema {
    cratestack::include_client_schema!("tests/fixtures/blog.cstack");
}

mod rpc_schema {
    cratestack::include_client_schema!("tests/fixtures/transport_rpc.cstack");
}

use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};

#[tokio::test]
async fn rest_client_lists_and_selects_and_calls_a_paged_procedure() {
    let (base_url, _server) = support::spawn_mock_server(|request| {
        if request.path == "/posts" && request.method == "GET" {
            return support::cbor_ok(&vec![rest_schema::cratestack_schema::Post {
                id: 1,
                title: "Hello".to_owned(),
                subtitle: None,
                published: true,
                authorId: 7,
            }]);
        }
        if request.path.starts_with("/$procs/getFeedPage") && request.method == "POST" {
            let page = cratestack::Page::new(
                vec![rest_schema::cratestack_schema::Post {
                    id: 2,
                    title: "Feed".to_owned(),
                    subtitle: None,
                    published: true,
                    authorId: 7,
                }],
                cratestack::PageInfo {
                    limit: Some(1),
                    offset: Some(0),
                    has_next_page: false,
                    has_previous_page: false,
                },
            )
            .with_total_count(Some(1));
            return support::cbor_ok(&page);
        }
        support::not_found()
    })
    .await;

    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = rest_schema::cratestack_schema::client::Client::new(runtime);

    let posts = client
        .posts()
        .list(&[], &[])
        .await
        .expect("list should succeed");
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title, "Hello");

    // Type-level proof that projection/`select()` codegen resolves under
    // this facade (`ProjectionDecoder` bound on `get_view`/`list_view`
    // comes from `::cratestack::ProjectionDecoder`, re-exported via the
    // `cratestack_core::*` glob) — not exercised over the wire here, just
    // constructed.
    let _selection = rest_schema::cratestack_schema::post::select()
        .id()
        .title()
        .include_author_selected(rest_schema::cratestack_schema::user::include_selection().email());

    let feed_page = client
        .procedures()
        .get_feed_page(
            &rest_schema::cratestack_schema::procedures::get_feed_page::Args {
                limit: Some(1),
                offset: Some(0),
            },
            &[],
        )
        .await
        .expect("paged procedure call should succeed");
    assert_eq!(feed_page.items.len(), 1);
    assert_eq!(feed_page.items[0].title, "Feed");
    assert_eq!(feed_page.total_count, Some(1));

    // `<Model>Where`/relation-path filter codegen (`FieldRef`/`FilterExpr`/
    // `wrap_filter`, all re-exported from `cratestack-sql`) resolves too —
    // build a filter through the generated field accessor and relation path
    // without sending it anywhere.
    let where_clause = rest_schema::cratestack_schema::PostWhere {
        published: Some(cratestack::FieldFilterInput {
            eq: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(where_clause.to_filters().len(), 1);
}

#[tokio::test]
async fn rpc_client_widget_crud_and_streaming_procedure_round_trip() {
    let (base_url, _server) = support::spawn_mock_server(|request| {
        if request.path == "/rpc/model.Widget.list" && request.method == "POST" {
            return support::cbor_ok(&vec![rpc_schema::cratestack_schema::Widget {
                id: 1,
                name: "Alpha".to_owned(),
            }]);
        }
        if request.path == "/rpc/procedure.many_pings" && request.method == "POST" {
            use cratestack_core::CratestackCodec;
            let mut body = Vec::new();
            for nonce in ["one", "two"] {
                let item = rpc_schema::cratestack_schema::PingArgs {
                    nonce: nonce.to_owned(),
                };
                body.extend(CborCodec.encode(&item).expect("item should encode"));
            }
            return support::MockResponse {
                status: 200,
                content_type: "application/cbor-seq".to_owned(),
                body,
                extra_headers: Vec::new(),
            };
        }
        support::not_found()
    })
    .await;

    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = rpc_schema::cratestack_schema::client::Client::new(runtime);

    let widgets = client
        .widgets()
        .list(&cratestack::rpc::RpcListInput::default())
        .await
        .expect("rpc list should succeed");
    assert_eq!(widgets.len(), 1);
    assert_eq!(widgets[0].name, "Alpha");

    let mut stream = client
        .procedures()
        .many_pings(
            &rpc_schema::cratestack_schema::procedures::many_pings::Args {
                args: rpc_schema::cratestack_schema::PingArgs {
                    nonce: "ignored".to_owned(),
                },
            },
        )
        .await
        .expect("streaming procedure call should open");
    let mut nonces = Vec::new();
    while let Some(item) = stream.recv().await {
        nonces.push(item.expect("stream item should decode").nonce);
    }
    assert_eq!(nonces, vec!["one".to_owned(), "two".to_owned()]);
}
