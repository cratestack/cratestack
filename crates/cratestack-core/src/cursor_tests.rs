use super::*;
use crate::{CursorPage, CursorPageInput};

#[test]
fn cursor_round_trips_values_including_null() {
    let values = vec![Some("2024-01-02T03:04:05Z".to_owned()), None, Some("42".to_owned())];
    let raw = encode_cursor("-createdAt,id", &values);
    assert!(!raw.contains('='), "base64url without padding: {raw}");
    assert_eq!(decode_cursor(&raw, "-createdAt,id", 3).unwrap(), values);
}

#[test]
fn cursor_for_a_different_order_is_rejected() {
    let raw = encode_cursor("id", &[Some("1".to_owned())]);
    let error = decode_cursor(&raw, "-id", 1).unwrap_err();
    assert!(matches!(error, CratestackError::BadRequest(m) if m.contains("different sort order")));
}

#[test]
fn malformed_or_oversized_cursors_are_bad_requests() {
    for raw in ["!!!not-base64!!!", "e30", "bm90LWpzb24"] {
        assert!(
            matches!(decode_cursor(raw, "id", 1), Err(CratestackError::BadRequest(_))),
            "{raw}"
        );
    }
    let huge = "A".repeat(MAX_CURSOR_LEN + 1);
    assert!(matches!(decode_cursor(&huge, "id", 1), Err(CratestackError::BadRequest(_))));
    // Wrong value count for the order is also malformed.
    let raw = encode_cursor("id", &[Some("1".to_owned())]);
    assert!(matches!(decode_cursor(&raw, "id", 2), Err(CratestackError::BadRequest(_))));
}

#[test]
fn cursor_scalar_renders_wire_scalars() {
    assert_eq!(cursor_scalar(&42_i64).unwrap(), Some("42".to_owned()));
    assert_eq!(cursor_scalar(&"a b").unwrap(), Some("a b".to_owned()));
    assert_eq!(cursor_scalar(&true).unwrap(), Some("true".to_owned()));
    assert_eq!(cursor_scalar(&Option::<i64>::None).unwrap(), None);
    assert!(cursor_scalar(&vec![1, 2]).is_err());
}

#[test]
fn cursor_page_from_rows_truncates_and_mints_next_cursor() {
    let page = CursorPage::from_rows(vec![1, 2, 3], 2, |last| format!("after-{last}"));
    assert_eq!(page.items, vec![1, 2]);
    assert_eq!(page.next_cursor.as_deref(), Some("after-2"));
    let page = CursorPage::from_rows(vec![1, 2], 2, |last| format!("after-{last}"));
    assert_eq!(page.items, vec![1, 2]);
    assert_eq!(page.next_cursor, None);
}

#[test]
fn cursor_page_input_resolves_limit_like_page_input() {
    let input = CursorPageInput { limit: Some(5000), cursor: Some("abc".to_owned()) };
    assert_eq!(input.resolve(1000), (1000, Some("abc")));
    // An empty cursor means "first page", same as an absent one.
    let input = CursorPageInput { limit: None, cursor: Some(String::new()) };
    assert_eq!(input.resolve(1000), (1000, None));
    let json = serde_json::to_string(&CursorPage::new(vec![1], Some("c".to_owned()))).unwrap();
    assert_eq!(json, r#"{"items":[1],"nextCursor":"c"}"#);
}
