//! The two `name` rules beyond a DNS label's (maintainer decisions on
//! cratestack#1040, 2026-09-25): IDNA's reserved `??--` form is refused, and
//! a name needs a letter. Same shape as `tests_mcp_name.rs`: one test per
//! rule, so deleting that rule's check fails that test, plus the order the
//! rules are checked in, for a name that breaks more than one.

use crate::parse_schema;
use crate::tests_mcp_support::{edit, syntax_error};

const NAME: &str = "  name = \"blog\"\n";
const RESERVED: &str = "must not have `--` as its 3rd and 4th characters, the form IDNA reserves \
                        for encoded labels like `xn--` (RFC 5891 § 4.2.3.1)";
const LETTER: &str = "must contain at least one letter";
const EDGE: &str = "must not start or end with `-`";

fn message(value: &str) -> String {
    syntax_error(&edit(NAME, &format!("  name = \"{value}\"\n")))
}

/// `xn--` is IDNA's ACE prefix, which an IDNA-aware client may show as a
/// different, Unicode string, and every other `??--` is reserved for a
/// prefix like it (RFC 5891 § 4.2.3.1).
#[test]
fn rule_the_name_does_not_have_hyphens_as_its_3rd_and_4th_characters() {
    for value in [
        "xn--blog",
        "xn--bcher-kva",
        "ab--c",
        "zz--9",
        "a1---b",
        "09--a",
    ] {
        let message = message(value);
        assert!(message.contains(RESERVED), "{value:?}: {message}");
    }
}

#[test]
fn rule_the_name_contains_a_letter() {
    for value in ["127", "2026", "0", "7", "1-2", "0-0-0"] {
        let message = message(value);
        assert!(message.contains(LETTER), "{value:?}: {message}");
    }
}

/// A `--` anywhere but the 3rd and 4th characters is not the reserved form,
/// and a letter anywhere satisfies the other rule.
#[test]
fn a_name_beside_the_two_rules_is_a_name() {
    for value in [
        "a--b", "ab-c--d", "abc--d", "abc---d", "3d-shop", "v2", "2v", "1a1", "x",
    ] {
        let source = edit(NAME, &format!("  name = \"{value}\"\n"));
        let schema = parse_schema(&source).unwrap_or_else(|error| panic!("{value:?}: {error}"));
        assert_eq!(schema.mcp.unwrap().name.unwrap().value, value);
    }
}

/// A name breaking several rules is told one, the most specific: the
/// character set, then the length (both before anything else, as
/// `broken_label_rule` explains), then the two exact positions IDNA
/// reserves, then either end, then the whole name's lack of a letter.
#[test]
fn a_name_breaking_several_rules_is_told_the_most_specific() {
    let long_digits = "1".repeat(64);
    for (value, rule) in [
        // Reserved and an edge: the positional rule, which a fix of the
        // edge alone (`xn--` to `xn--a`) would still break.
        ("xn--", RESERVED),
        ("ab--", RESERVED),
        // Reserved and no letter.
        ("12--3", RESERVED),
        // An edge and no letter.
        ("-127", EDGE),
        ("127-", EDGE),
        // Broken before either new rule is looked at.
        ("XN--blog", "must be lowercase ASCII letters"),
        ("ab--\u{e9}", "must be lowercase ASCII letters"),
        (long_digits.as_str(), "must be 1 to 63 characters"),
    ] {
        let message = message(value);
        assert!(message.contains(rule), "{value:?}: {message}");
    }
}
