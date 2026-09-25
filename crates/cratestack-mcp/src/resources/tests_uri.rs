//! URI matching: exactly the two shapes, nothing normalized but the
//! scheme's case.

use cratestack_core::{OpDescriptor, OpKind};

use super::ResourceDescriptor;
use super::uri::{Target, UriError, parse};

static OP: OpDescriptor = OpDescriptor {
    op_id: "model.Post.get",
    kind: OpKind::Unary,
    input_ty: "",
    output_ty: "",
    idempotent_by_default: true,
    rate_limited_by_default: true,
    auth_required: false,
};

static TABLE: [ResourceDescriptor; 2] = [
    ResourceDescriptor::new("blog", "posts", 200, &OP, &OP),
    ResourceDescriptor::new("blog", "comments", 20, &OP, &OP),
];

fn record(uri: &str) -> (&'static str, String) {
    match parse(uri, &TABLE) {
        Ok(Target::Record { resource, id }) => (resource.segment, id),
        other => panic!("{uri}: expected a record, got {other:?}"),
    }
}

fn page(uri: &str) -> (&'static str, Option<u64>, Option<String>) {
    match parse(uri, &TABLE) {
        Ok(Target::Page {
            resource,
            limit,
            cursor,
        }) => (resource.segment, limit, cursor),
        other => panic!("{uri}: expected a page, got {other:?}"),
    }
}

fn error(uri: &str) -> UriError {
    match parse(uri, &TABLE) {
        Err(error) => error,
        Ok(target) => panic!("{uri}: expected an error, got {target:?}"),
    }
}

#[test]
fn a_record_uri_names_the_resource_and_the_decoded_id() {
    assert_eq!(
        record("cratestack://blog/posts/7"),
        ("posts", "7".to_owned())
    );
    assert_eq!(
        record("cratestack://blog/comments/a%2Fb%20c"),
        ("comments", "a/b c".to_owned())
    );
}

#[test]
fn a_collection_uri_carries_only_limit_and_cursor() {
    assert_eq!(page("cratestack://blog/posts"), ("posts", None, None));
    assert_eq!(
        page("cratestack://blog/posts?limit=500&cursor=abc"),
        ("posts", Some(500), Some("abc".to_owned()))
    );
    assert_eq!(
        page("cratestack://blog/posts?limit=99999999999999999999999"),
        ("posts", Some(u64::MAX), None),
        "an absurd limit saturates, to be clamped rather than refused"
    );
}

#[test]
fn anything_else_is_unknown() {
    for uri in [
        "cratestack://blog/Post/1",
        "cratestack://blog/post_table/1",
        "cratestack://other/posts",
        // The name, segment and id are matched exactly, whatever the
        // scheme's case (`the_scheme_is_matched_in_any_case`).
        "cratestack://BLOG/posts",
        "CRATESTACK://BLOG/posts",
        "cratestack://Blog/posts/1",
        "CRATESTACK://blog/POSTS/1",
        // A multi-byte character across the scheme's end is a mismatch,
        // not a panic.
        "cratestacé://blog/posts",
        "cratestack://blog/posts/",
        "cratestack://blog/posts/1/2",
        "cratestack://blog/posts#x",
        "cratestack://blog",
        "file://blog/posts",
        "cratestack:/blog/posts",
        "cratestack://blog/posts/%zz",
        "cratestack://blog/posts/%+1",
        "cratestack://blog/posts/%ff",
        // No key of any addressable type can hold a NUL: `Int`/`Uuid`
        // never parse one, and Postgres refuses it in `text` before
        // comparing a row — as a database error, not "not found".
        "cratestack://blog/comments/a%00b",
        "cratestack://blog/comments/%00",
    ] {
        assert_eq!(error(uri), UriError::Unknown, "{uri}");
    }
}

/// RFC 3986 § 3.1: a scheme is case-insensitive (maintainer decision on
/// #1040, which flipped the lowercase-only pin `anything_else_is_unknown`
/// used to carry). Only the scheme: the name after it stays exact.
#[test]
fn the_scheme_is_matched_in_any_case() {
    for scheme in ["CRATESTACK", "Cratestack", "cRaTeStAcK"] {
        assert_eq!(
            record(&format!("{scheme}://blog/posts/7")),
            ("posts", "7".to_owned()),
            "{scheme}"
        );
        assert_eq!(
            page(&format!("{scheme}://blog/comments?limit=5")),
            ("comments", Some(5), None),
            "{scheme}"
        );
        assert_eq!(
            error(&format!("{scheme}://BLOG/posts/7")),
            UriError::Unknown,
            "{scheme}: the name is not case-folded"
        );
    }
}

/// Only *ASCII* case folds, and nothing else about the scheme is forgiven.
/// Each of these survived as a mutation of `strip_scheme`: Unicode
/// lowercasing (U+212A KELVIN SIGN lowercases to ASCII `k`, so
/// `CRATESTAC\u{212A}` would pass `to_lowercase`), trimming whitespace, and
/// accepting `cratestack:` without the `//`.
#[test]
fn the_scheme_folds_ascii_case_and_nothing_else() {
    for uri in [
        "CRATESTAC\u{212A}://blog/posts/1",
        "cratestac\u{212A}://blog/posts/1",
        "CRATE\u{17F}TACK://blog/posts/1",
        " cratestack://blog/posts/1",
        "\tcratestack://blog/posts/1",
        "cratestack ://blog/posts/1",
        "cratestack:// blog/posts/1",
        "cratestack:blog/posts/1",
        "cratestack:blog/posts",
        "CRATESTACK:blog/posts",
    ] {
        assert_eq!(error(uri), UriError::Unknown, "{uri:?}");
    }
}

#[test]
fn a_bad_page_query_is_invalid_not_unknown() {
    for uri in [
        "cratestack://blog/posts?limit=0",
        "cratestack://blog/posts?limit=-1",
        "cratestack://blog/posts?limit=ten",
        "cratestack://blog/posts?limit=",
        "cratestack://blog/posts?cursor=",
        "cratestack://blog/posts?limit=1&limit=2",
        "cratestack://blog/posts?offset=10",
        "cratestack://blog/posts/1?limit=5",
    ] {
        assert!(
            matches!(error(uri), UriError::Invalid(_)),
            "{uri} should be refused as invalid"
        );
    }
}
