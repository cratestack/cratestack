//! `error::from_cratestack`'s rule 1, pinned: a `NotFound` or `Forbidden`
//! coming back from the read path is the one constant not-found error, byte
//! for byte. The hand-written table in `tests/resources.rs` only ever
//! answers `Ok(None)`, so without this, dropping either arm of that match
//! (mutation: `NotFound(_) | Forbidden(_)` narrowed to one of them) passed
//! every test — and a `Forbidden` for a row that exists, answered
//! differently from a missing one, is the existence oracle security
//! requirement 12 forbids.

use cratestack_core::CratestackError;

use super::error::{from_cratestack, not_found};

#[test]
fn not_found_and_forbidden_from_the_read_path_are_the_constant_not_found() {
    let expected = serde_json::to_value(not_found()).unwrap();
    for failure in [
        CratestackError::NotFound("McpResPost 3 not found".to_owned()),
        CratestackError::Forbidden("row 3 belongs to u-2".to_owned()),
    ] {
        let answered =
            serde_json::to_value(from_cratestack("cratestack://blog/posts/3", failure)).unwrap();
        assert_eq!(answered, expected, "nothing about the row may differ");
    }
    assert_eq!(expected["code"], -32602);
    assert!(expected.get("data").is_none(), "{expected}");
}

#[test]
fn an_internal_failure_carries_only_the_public_envelope() {
    let answered = serde_json::to_value(from_cratestack(
        "cratestack://blog/posts/3",
        CratestackError::Internal("relation mcp_res_posts does not exist".to_owned()),
    ))
    .unwrap();
    assert_eq!(answered["code"], -32603);
    let text = answered.to_string();
    assert!(!text.contains("mcp_res_posts"), "{text}");
    assert!(!text.contains("posts/3"), "{text}");
}
