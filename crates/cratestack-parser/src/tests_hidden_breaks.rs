//! A character an editor shows as a line break, but the parser does not
//! split on, hid a policy attribute inside the `//` comment before it
//! (GHSA-69g4-xvcm-vm2j review; `parse::hidden_breaks`). Measured before
//! the refusal: `@allow(auth() != null) // note` + lone CR +
//! `@deny(hasRole("banned"))` parsed with no deny rule, and the generated
//! procedure answered `200 OK` to a `banned` caller.

use crate::parse_schema;

const HEAD: &str = "auth SessionUser {\n  id Int\n  role String\n}\n\n\
                    type Args {\n  n Int\n}\n\n\
                    type Summary {\n  total Int\n}\n";

const BREAKS: [char; 6] = ['\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}'];

#[track_caller]
fn refused(body: &str, needle: &str) {
    let source = format!("{HEAD}\n{body}");
    let message = parse_schema(&source)
        .err()
        .unwrap_or_else(|| panic!("must be refused, but parsed:\n{body:?}"))
        .to_string();
    assert!(message.contains(needle), "missing {needle:?}: {message}");
}

#[test]
fn a_hidden_break_after_a_comment_is_refused() {
    for ch in BREAKS {
        let code = format!("U+{:04X}", ch as u32);
        for body in [
            // The deny would be the tail of the allow line's comment.
            format!(
                "procedure p(args: Args): Summary\n  @allow(auth() != null) // all{ch}  \
                 @deny(hasRole(\"banned\"))\n"
            ),
            // The deny would be the tail of a comment line (pre-existing).
            format!(
                "procedure p(args: Args): Summary\n  @allow(auth() != null)\n  // banned{ch}  \
                 @deny(hasRole(\"banned\"))\n"
            ),
            format!(
                "query q(n: Int): Summary\n  @@sql(\"SELECT $1::bigint AS total\")\n  \
                 @allow(true) // all{ch}  @deny(hasRole(\"banned\"))\n"
            ),
            format!(
                "model M {{\n  id Int @id\n  @@allow(\"all\", true) // all{ch}  \
                 @@deny(\"all\", auth().role == \"banned\")\n}}\n"
            ),
            format!(
                "model M {{\n  id Int @id\n  owner Int // x{ch}  @@deny(\"all\", true)\n  \
                 @@allow(\"all\", true)\n}}\n"
            ),
        ] {
            refused(&body, &code);
        }
    }
}

#[test]
fn a_bidirectional_control_is_refused_anywhere() {
    for ch in ['\u{202A}', '\u{202E}', '\u{2066}', '\u{2069}'] {
        refused(
            &format!("model M {{\n  id Int @id\n  @@allow(\"all\", true) // {ch}note\n}}\n"),
            "bidirectional text control",
        );
        refused(
            &format!("model M {{\n  id Int @id @default(\"a{ch}b\")\n}}\n"),
            "bidirectional text control",
        );
    }
}

/// ESC and CSI start a terminal escape sequence, which can draw a comment
/// over with text that looks applied; refused anywhere, comments included.
#[test]
fn a_terminal_escape_is_refused_anywhere() {
    for ch in ['\u{1B}', '\u{9B}'] {
        let code = format!("U+{:04X}", ch as u32);
        for body in [
            format!(
                "procedure p(args: Args): Summary\n  @allow(true)\n  // {ch}[3D@deny(hasRole(\"banned\"))\n"
            ),
            format!("model M {{\n  id Int @id\n  @@allow(\"all\", true) // {ch}[8mnote\n}}\n"),
            format!("model M {{\n  id Int @id @default(\"a{ch}b\")\n}}\n"),
        ] {
            refused(&body, &code);
            refused(&body, "terminal escape sequence");
        }
    }
}

// Positive controls: `\r\n` line endings, and a break character that ends
// its line (nothing after it to hide).
#[test]
fn ordinary_and_trailing_breaks_still_parse() {
    let body = "procedure p(args: Args): Summary\n  @allow(auth() != null) // all\n  \
                @deny(hasRole(\"banned\"))\n";
    let crlf = format!("{HEAD}\n{body}").replace('\n', "\r\n");
    let schema = parse_schema(&crlf).unwrap_or_else(|error| panic!("CRLF parses: {error}"));
    assert_eq!(schema.procedures[0].attributes.len(), 2);
    for ch in BREAKS {
        let trailing = format!("{HEAD}\n{}", body.replace("// all", &format!("// all{ch}")));
        let schema = parse_schema(&trailing)
            .unwrap_or_else(|error| panic!("a trailing U+{:04X} parses: {error}", ch as u32));
        assert_eq!(schema.procedures[0].attributes.len(), 2);
    }
}
