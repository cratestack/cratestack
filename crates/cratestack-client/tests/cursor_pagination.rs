//! Rust client surface for keyset pagination (cratestack#1304) through the
//! `cratestack-client` facade: the `cursor` query pair reaches the server on a
//! `@@paged` list, `PageInfo::next_cursor` decodes, and a `CursorPage<T>`
//! procedure result decodes with `CursorPageInput` as its argument.

mod support;

mod schema {
    cratestack::include_client_schema!("tests/fixtures/cursor_pagination.cstack");
}

use cratestack::{CursorPage, CursorPageInput, Page, PageInfo};
use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};
use schema::cratestack_schema::{Entry, Note};

#[tokio::test]
async fn paged_list_sends_cursor_and_decodes_next_cursor() {
    let (base_url, _server) = support::spawn_mock_server(|request| {
        if request.method == "GET" && request.path.starts_with("/notes") {
            let next = request
                .path
                .contains("cursor=abc")
                .then_some("def".to_owned());
            let page = Page::new(
                vec![Note {
                    id: 1,
                    label: "n".to_owned(),
                }],
                PageInfo {
                    limit: Some(1),
                    has_next_page: next.is_some(),
                    next_cursor: next,
                    ..PageInfo::default()
                },
            );
            return support::cbor_ok(&page);
        }
        support::not_found()
    })
    .await;
    let client = schema::cratestack_schema::client::Client::new(CratestackClient::new(
        ClientConfig::new(base_url),
        CborCodec,
    ));

    let first = client
        .notes()
        .list(&[("cursor", "abc"), ("limit", "1")], &[])
        .await
        .expect("list");
    assert_eq!(first.page_info.next_cursor.as_deref(), Some("def"));
    let last = client
        .notes()
        .list(&[("cursor", "def")], &[])
        .await
        .expect("list");
    assert_eq!(last.page_info.next_cursor, None);
}

#[tokio::test]
async fn cursor_page_procedure_round_trips_input_and_result() {
    let (base_url, _server) = support::spawn_mock_server(|request| {
        if request.method == "POST" && request.path.starts_with("/$procs/feed") {
            let page = CursorPage::new(
                vec![Entry {
                    id: 1,
                    label: "e".to_owned(),
                }],
                Some("next".to_owned()),
            );
            return support::cbor_ok(&page);
        }
        support::not_found()
    })
    .await;
    let client = schema::cratestack_schema::client::Client::new(CratestackClient::new(
        ClientConfig::new(base_url),
        CborCodec,
    ));

    let page = client
        .procedures()
        .feed(
            &schema::cratestack_schema::procedures::feed::Args {
                page: CursorPageInput {
                    limit: Some(1),
                    cursor: None,
                },
            },
            &[],
        )
        .await
        .expect("feed");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.next_cursor.as_deref(), Some("next"));
}
