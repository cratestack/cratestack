//! `synthesize_get_query` tests.

#![cfg(test)]

use super::synthesize::synthesize_get_query;

#[test]
fn synthesize_get_query_returns_none_when_absent() {
    assert!(synthesize_get_query(None).is_none());
}

#[test]
fn synthesize_get_query_round_trips_through_parse_query_pairs() {
    let raw = r#"{"proxyUrl":{"width":800}}"#;
    let query =
        synthesize_get_query(Some(raw)).expect("computed params present, query should exist");
    let pairs = crate::parse_query_pairs(Some(&query)).expect("synthesized query parses");

    // Exactly one pair — `parse_model_fetch_query` hard-rejects any key it
    // doesn't recognize, so this must never leak extra pairs the way
    // `synthesize_list_query` legitimately does for its own handler.
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].0, "computedParams");
    assert_eq!(pairs[0].1, raw);
}
