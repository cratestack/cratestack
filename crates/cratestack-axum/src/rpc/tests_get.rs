//! `synthesize_get_query` tests — verify RPC `get` selection surface mirrors
//! REST `GET /<plural>/{id}` query parameter handling.

#![cfg(test)]

use cratestack_core::rpc::RpcGetInput;

use super::synthesize::synthesize_get_query;
use crate::parse_query_pairs;

fn input(id: i64) -> RpcGetInput<i64> {
    RpcGetInput {
        id,
        ..Default::default()
    }
}

#[test]
fn synthesize_get_query_returns_none_when_every_field_is_unset() {
    assert!(synthesize_get_query(&input(1)).is_none());
}

#[test]
fn synthesize_get_query_round_trips_computed_params_through_parse_query_pairs() {
    let raw = r#"{"proxyUrl":{"width":800}}"#;
    let query = synthesize_get_query(&RpcGetInput {
        id: 1i64,
        computed_params: Some(raw.to_owned()),
        ..Default::default()
    })
    .expect("computed params present, query should exist");
    let pairs = parse_query_pairs(Some(&query)).expect("synthesized query parses");

    // Exactly one pair — `parse_model_fetch_query` hard-rejects any key it
    // doesn't recognize, so this must never leak extra pairs the way
    // `synthesize_list_query` legitimately does for its own handler.
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].0, "computedParams");
    assert_eq!(pairs[0].1, raw);
}

#[test]
fn synthesize_get_query_emits_fields_include_and_include_fields() {
    let query = synthesize_get_query(&RpcGetInput {
        id: 1i64,
        fields: Some(vec!["id".into(), "name".into()]),
        include: Some(vec!["author".into()]),
        include_fields: std::collections::BTreeMap::from([(
            "author".to_owned(),
            vec!["id".to_owned(), "name".to_owned()],
        )]),
        computed_params: Some(r#"{"proxyUrl":{"width":800}}"#.to_owned()),
    })
    .expect("all fields present, query should exist");
    let pairs = parse_query_pairs(Some(&query)).expect("synthesized query parses");

    assert_eq!(
        pairs,
        vec![
            ("fields".to_owned(), "id,name".to_owned()),
            ("include".to_owned(), "author".to_owned()),
            ("includeFields[author]".to_owned(), r#"id,name"#.to_owned()),
            (
                "computedParams".to_owned(),
                r#"{"proxyUrl":{"width":800}}"#.to_owned()
            ),
        ]
    );
}

#[test]
fn synthesize_get_query_emits_the_same_pairs_synthesize_list_query_does() {
    use super::synthesize::synthesize_list_query;
    use cratestack_core::rpc::RpcListInput;

    let get = RpcGetInput {
        id: 1i64,
        fields: Some(vec!["id".into(), "name".into()]),
        include: Some(vec!["author".into()]),
        include_fields: std::collections::BTreeMap::from([(
            "author".to_owned(),
            vec!["id".to_owned(), "name".to_owned()],
        )]),
        computed_params: Some(r#"{"proxyUrl":{"width":800}}"#.to_owned()),
    };
    let list = RpcListInput {
        fields: Some(vec!["id".into(), "name".into()]),
        include: Some(vec!["author".into()]),
        include_fields: std::collections::BTreeMap::from([(
            "author".to_owned(),
            vec!["id".to_owned(), "name".to_owned()],
        )]),
        computed_params: Some(r#"{"proxyUrl":{"width":800}}"#.to_owned()),
        ..Default::default()
    };

    assert_eq!(synthesize_get_query(&get), synthesize_list_query(&list));
}
