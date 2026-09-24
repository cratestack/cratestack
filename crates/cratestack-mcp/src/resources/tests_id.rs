//! A record id is one RFC 3986 path segment (maintainer decision on #1040):
//! a raw character a URI may not carry there is "not found", and its
//! percent-encoded form is still the character.

use cratestack_core::{OpDescriptor, OpKind};

use super::ResourceDescriptor;
use super::uri::{Target, UriError, parse};

static OP: OpDescriptor = OpDescriptor {
    op_id: "model.Note.get",
    kind: OpKind::Unary,
    input_ty: "",
    output_ty: "",
    idempotent_by_default: true,
    rate_limited_by_default: true,
    auth_required: false,
};

static TABLE: [ResourceDescriptor; 1] = [ResourceDescriptor::new("blog", "notes", 20, &OP, &OP)];

fn id(uri: &str) -> Result<String, UriError> {
    match parse(uri, &TABLE)? {
        Target::Record { id, .. } => Ok(id),
        Target::Page { .. } => panic!("{uri}: a page, not a record"),
    }
}

/// Everything outside `pchar` that can sit inside one segment: controls,
/// space, DEL, the characters RFC 3986 § 2 never allows raw (`"`, `<`, `>`,
/// `\`, `^`, `` ` ``, `{`, `|`, `}`), `[` and `]` (host-only), and any
/// non-ASCII character, which only an IRI may carry raw. `/`, `?` and `#`
/// end the segment and are the matcher's own rules (`tests_uri.rs`).
#[test]
fn a_raw_character_outside_a_path_segment_is_not_found() {
    let refused = [
        " ", "\t", "\n", "\r", "\0", "\u{1}", "\u{1f}", "\u{7f}", "\"", "<", ">", "\\", "^", "`",
        "{", "|", "}", "[", "]", "é", "\u{a0}", "\u{202e}", "💥",
    ];
    for raw in refused {
        for shape in [format!("a{raw}b"), raw.to_owned(), format!("{raw}1")] {
            let uri = format!("cratestack://blog/notes/{shape}");
            assert_eq!(id(&uri), Err(UriError::Unknown), "{uri:?}");
        }
    }
}

/// `pchar` = unreserved / pct-encoded / sub-delims / `:` / `@` (RFC 3986
/// § 3.3), and every one of those still reaches the table as itself.
#[test]
fn every_path_segment_character_is_still_an_id() {
    let pchar = "AZaz09-._~!$&'()*+,;=:@";
    assert_eq!(
        id(&format!("cratestack://blog/notes/{pchar}")),
        Ok(pchar.to_owned())
    );
    for one in pchar.chars() {
        let uri = format!("cratestack://blog/notes/{one}");
        assert_eq!(id(&uri), Ok(one.to_string()), "{uri}");
    }
}

#[test]
fn a_percent_encoded_character_is_the_character() {
    for (encoded, decoded) in [
        ("a%20b", "a b"),
        ("%22%3C%3E%5C%5E%60%7B%7C%7D%5B%5D", "\"<>\\^`{|}[]"),
        ("caf%C3%A9", "café"),
        ("tab%09", "tab\t"),
        ("%F0%9F%92%A5", "💥"),
    ] {
        let uri = format!("cratestack://blog/notes/{encoded}");
        assert_eq!(id(&uri), Ok(decoded.to_owned()), "{uri}");
    }
}
