//! The cursor round-trips, and anything this server did not mint for the
//! resource is refused: an edited digit, an edited offset with the old tag,
//! a cursor from another resource, truncation, another spelling.

use super::cursor::{decode, encode};

#[test]
fn a_minted_cursor_round_trips() {
    for offset in [0, 1, 50, 200, 12_345, i64::MAX as u64] {
        let cursor = encode("posts", offset);
        assert_eq!(decode("posts", &cursor), Some(offset), "{cursor}");
    }
}

#[test]
fn every_single_digit_edit_is_refused() {
    let cursor = encode("posts", 50);
    for at in 0..cursor.len() {
        let mut edited = cursor.clone().into_bytes();
        edited[at] = if edited[at] == b'0' { b'1' } else { b'0' };
        let edited = String::from_utf8(edited).unwrap();
        assert_eq!(decode("posts", &edited), None, "digit {at}: {edited}");
    }
}

#[test]
fn a_rewritten_offset_without_its_tag_is_refused() {
    let cursor = encode("posts", 50);
    // Bytes 1..9 are the offset: swap in 40 and keep the old tag.
    let forged = format!("{}{:016x}{}", &cursor[..2], 40_u64, &cursor[18..]);
    assert_eq!(forged.len(), cursor.len());
    assert_eq!(decode("posts", &forged), None);
}

#[test]
fn a_cursor_is_bound_to_its_resource() {
    let cursor = encode("posts", 50);
    assert_eq!(decode("comments", &cursor), None);
}

#[test]
fn malformed_cursors_are_refused() {
    let cursor = encode("posts", 50);
    for bad in [
        "",
        "not-a-cursor",
        &cursor[..cursor.len() - 2],
        &format!("{cursor}00"),
        &cursor.to_uppercase(),
    ] {
        assert_eq!(decode("posts", bad), None, "{bad:?}");
    }
}

#[test]
fn an_offset_the_orm_cannot_bind_is_refused() {
    let cursor = encode("posts", u64::MAX);
    assert_eq!(decode("posts", &cursor), None);
}
