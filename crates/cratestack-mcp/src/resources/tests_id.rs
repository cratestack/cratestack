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

/// Escapes are decoded exactly once: `%2541` is the three characters `%41`,
/// never `A`. Decoding twice would give one record a second spelling and
/// let an escaped `%` smuggle a character past the raw check above.
#[test]
fn an_escape_is_decoded_once() {
    for (encoded, decoded) in [("%2541", "%41"), ("%25", "%"), ("%2520", "%20")] {
        let uri = format!("cratestack://blog/notes/{encoded}");
        assert_eq!(id(&uri), Ok(decoded.to_owned()), "{uri}");
    }
}

/// `pct-encoded = "%" HEXDIG HEXDIG`, and the decoded bytes must be UTF-8:
/// a `%` without two hex digits after it (at the end too), an overlong or
/// surrogate encoding, a lone lead or continuation byte, and a decoded NUL
/// address no record. A decoded `/` is only a character of the id, never a
/// second segment.
#[test]
fn a_malformed_escape_or_bytes_that_are_not_utf8_are_not_found() {
    for raw in [
        "%",
        "a%",
        "a%4",
        "%4",
        "%G1",
        "%1G",
        "%%41",
        "%C0%AF",
        "%E0%80%AF",
        "%ED%A0%80",
        "%80",
        "%C3",
        "%00",
        "a%00",
    ] {
        let uri = format!("cratestack://blog/notes/{raw}");
        assert_eq!(id(&uri), Err(UriError::Unknown), "{uri}");
    }
    assert_eq!(id("cratestack://blog/notes/a%2fb"), Ok("a/b".to_owned()));
}
