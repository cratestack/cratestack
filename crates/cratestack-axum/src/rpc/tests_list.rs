//! `RpcListInput` synthesis tests.

#![cfg(test)]

use super::inputs::{RpcListInput, RpcListPredicate};
use super::synthesize::synthesize_list_query;

#[test]
fn synthesize_list_query_returns_none_when_empty() {
    let input = RpcListInput::default();
    assert!(synthesize_list_query(&input).is_none());
}

#[test]
fn synthesize_list_query_round_trips_through_parse_query_pairs() {
    let mut include_fields = std::collections::BTreeMap::new();
    include_fields.insert(
        "author".to_owned(),
        vec!["id".to_owned(), "name".to_owned()],
    );

    let input = RpcListInput {
        limit: Some(20),
        offset: Some(40),
        fields: Some(vec!["id".to_owned(), "title".to_owned()]),
        include: Some(vec!["author".to_owned()]),
        include_fields,
        sort: Some("createdAt desc".to_owned()),
        where_expr: Some("published=true".to_owned()),
        or: None,
        filters: vec![RpcListPredicate {
            key: "authorId".to_owned(),
            value: "42".to_owned(),
        }],
    };

    let query = synthesize_list_query(&input).expect("input not empty, query should exist");
    let pairs = crate::parse_query_pairs(Some(&query)).expect("synthesized query parses");

    // Every input field re-appears in the parsed pairs with the right key.
    // The parser strips no information, so this is a faithful round-trip.
    let has = |k: &str, v: &str| pairs.iter().any(|(pk, pv)| pk == k && pv == v);
    assert!(has("limit", "20"));
    assert!(has("offset", "40"));
    assert!(has("fields", "id,title"));
    assert!(has("include", "author"));
    assert!(has("includeFields[author]", "id,name"));
    assert!(has("sort", "createdAt desc"));
    assert!(has("where", "published=true"));
    assert!(has("authorId", "42"));
}
