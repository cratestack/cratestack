use std::sync::Arc;

use cratestack_core::Schema;

use super::transport::{detail_url, list_url};
use super::*;
use crate::config::ApiAuth;
use crate::data::{DataSource, PageRequest, PkCast};

fn empty_schema() -> Arc<Schema> {
    Arc::new(
        cratestack_parser::parse_schema("model Probe {\n  id String @id\n}\n")
            .expect("trivial schema parses"),
    )
}

#[test]
fn builds_bearer_header() {
    let source = ApiSource::new(
        "https://example.test".to_owned(),
        Some(&ApiAuth::Bearer {
            token: "abc".to_owned(),
        }),
        empty_schema(),
    )
    .expect("client builds");
    assert_eq!(
        source.auth_header,
        Some(("Authorization".to_owned(), "Bearer abc".to_owned()))
    );
}

#[test]
fn builds_custom_header() {
    let source = ApiSource::new(
        "https://example.test".to_owned(),
        Some(&ApiAuth::Header {
            name: "X-Api-Key".to_owned(),
            value: "k123".to_owned(),
        }),
        empty_schema(),
    )
    .expect("client builds");
    assert_eq!(
        source.auth_header,
        Some(("X-Api-Key".to_owned(), "k123".to_owned()))
    );
}

#[test]
fn list_url_pluralizes_and_snake_cases_model() {
    assert_eq!(
        list_url("https://example.test", "OrderItem"),
        "https://example.test/api/order_items"
    );
}

#[test]
fn list_url_strips_trailing_slash() {
    assert_eq!(
        list_url("https://example.test/", "Post"),
        "https://example.test/api/posts"
    );
}

#[test]
fn detail_url_percent_encodes_pk_segment() {
    assert_eq!(
        detail_url("https://example.test", "Post", "abc/123 xy"),
        "https://example.test/api/posts/abc%2F123%20xy"
    );
}

#[tokio::test]
async fn follow_returns_unsupported() {
    let source = ApiSource::new("https://x".to_owned(), None, empty_schema()).expect("client");
    let error = source
        .follow("M", "c", PkCast::Text, "v", PageRequest::default())
        .await
        .expect_err("unsupported");
    assert!(matches!(error, DataError::Unsupported { .. }));
}
