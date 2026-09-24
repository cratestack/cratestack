//! Q3's page-size rule, pinned where it lives. Removing the clamp in
//! `page.rs` fails `a_larger_request_is_clamped_not_refused`; losing the
//! per-resource ceiling fails `max_page_size_lowers_the_ceiling`.

use super::DEFAULT_PAGE_SIZE;
use super::page::page_size;

#[test]
fn the_default_is_fifty() {
    assert_eq!(DEFAULT_PAGE_SIZE, 50);
    assert_eq!(page_size(None, 200), 50);
}

#[test]
fn a_larger_request_is_clamped_not_refused() {
    assert_eq!(page_size(Some(500), 200), 200);
    assert_eq!(page_size(Some(201), 200), 200);
    assert_eq!(page_size(Some(u64::MAX), 200), 200);
    assert_eq!(page_size(Some(200), 200), 200);
    assert_eq!(page_size(Some(7), 200), 7);
}

#[test]
fn max_page_size_lowers_the_ceiling_and_the_default() {
    assert_eq!(page_size(Some(500), 20), 20);
    assert_eq!(page_size(Some(21), 20), 20);
    assert_eq!(
        page_size(None, 20),
        20,
        "the default never exceeds the ceiling"
    );
    assert_eq!(page_size(None, 80), 50);
}

#[test]
fn a_descriptor_cannot_raise_the_ceiling_past_two_hundred() {
    assert_eq!(page_size(Some(500), 1000), 200);
    assert_eq!(
        page_size(Some(500), 0),
        1,
        "a zero maximum is held to 1, not 0"
    );
}
