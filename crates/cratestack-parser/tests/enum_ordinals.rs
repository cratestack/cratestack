//! Explicit enum ordinals (`docs/design/enum-ordinals.md`): the body syntax
//! and rules R1–R5.

use cratestack_core::Schema;
use cratestack_parser::{parse_schema, parse_schema_unvalidated};

const PAYMENT: &str = "enum PaymentStatus {
  Pending = 1
  Approved = 2 // was Accepted
  Rejected=3
  Refunded = 5

  @@reserved(4)   // was Settled; never reuse
}
";

fn unvalidated(source: &str) -> Schema {
    parse_schema_unvalidated(source).unwrap_or_else(|error| panic!("must parse: {error}"))
}

/// Refused, the message has every needle, and the span is exactly `at`.
#[track_caller]
fn refused_at(source: &str, at: &str, needles: &[&str]) {
    let error = parse_schema(source)
        .err()
        .unwrap_or_else(|| panic!("must be refused:\n{source}"));
    let message = error.to_string();
    for needle in needles {
        assert!(message.contains(needle), "missing {needle:?}: {message}");
    }
    assert_eq!(&source[error.span()], at, "{message}");
}

fn payment_with(body: &str) -> String {
    format!("enum PaymentStatus {{\n{body}\n}}\n")
}

#[test]
fn an_ordinal_enum_reads_into_the_ir() {
    let schema = unvalidated(PAYMENT);
    let e = &schema.enums[0];
    let pairs: Vec<_> = e
        .variants
        .iter()
        .map(|v| (v.name.as_str(), v.ordinal))
        .collect();
    assert_eq!(
        pairs,
        [
            ("Pending", Some(1)),
            ("Approved", Some(2)),
            ("Rejected", Some(3)),
            ("Refunded", Some(5)),
        ]
    );
    assert_eq!(e.reserved.as_ref().map(|r| r.values.clone()), Some(vec![4]));
    // A variant's span is its name, as it is on a string enum.
    assert_eq!(
        &PAYMENT[e.variants[1].span.start..e.variants[1].span.end],
        "Approved"
    );
}

#[test]
fn a_well_formed_ordinal_enum_is_accepted() {
    let schema = parse_schema(PAYMENT).expect("a well-formed ordinal enum validates");
    let e = &schema.enums[0];
    assert!(e.is_ordinal());
    assert_eq!(e.ordinal_of("Refunded"), Some(5));
    assert_eq!(e.ordinal_of("Settled"), None);
}

#[test]
fn r5_an_auth_field_is_never_an_ordinal_enum() {
    let source = format!("{PAYMENT}\nauth Claims {{\n  id Int\n  status PaymentStatus\n}}\n");
    refused_at(
        &source,
        // A field's span starts at its line's indent.
        "  status PaymentStatus",
        &["auth field `status`", "ordinal enum type `PaymentStatus`"],
    );
    let string_enum = "enum Role {\n  admin\n}\n\nauth Claims {\n  id Int\n  role Role\n}\n";
    parse_schema(string_enum).expect("a string enum stays a valid auth field type");
}

#[test]
fn a_string_enum_is_unchanged() {
    let schema = parse_schema("enum Role {\n  admin\n  member\n}\n").expect("parses");
    let e = &schema.enums[0];
    assert!(e.reserved.is_none());
    assert!(e.variants.iter().all(|v| v.ordinal.is_none()));
    // The IR's serialized form gains no key, so nothing hashing it moves.
    let json = serde_json::to_string(e).unwrap();
    assert!(
        !json.contains("ordinal") && !json.contains("reserved"),
        "{json}"
    );
}

#[test]
fn r1_every_variant_is_numbered_or_none_is() {
    let source = payment_with("  Pending = 1\n  Approved");
    refused_at(
        &source,
        "Approved",
        &["numbers some variants but not `Approved`"],
    );
}

#[test]
fn r2_an_ordinal_is_in_range_and_unique() {
    for (value, shown) in [("-1", "-1 is not"), ("32768", "32768 is not")] {
        let source = payment_with(&format!("  Pending = {value}"));
        refused_at(&source, "Pending", &["from 0 to 32767", shown]);
    }
    let source = payment_with("  Pending = 1\n  Approved = 1");
    refused_at(
        &source,
        "Approved",
        &["ordinal 1 is used by both `Pending` and `Approved`"],
    );
}

#[test]
fn r3_a_reserved_ordinal_is_never_reused() {
    let source = payment_with("  Pending = 1\n  Approved = 4\n  @@reserved(4)");
    refused_at(
        &source,
        "@@reserved(4)",
        &["`Approved` uses ordinal 4", "never reused"],
    );
    let source = payment_with("  Pending = 1\n  @@reserved(4, 4)");
    refused_at(&source, "@@reserved(4, 4)", &["lists 4 twice"]);
    let source = payment_with("  Pending = 1\n  @@reserved(40000)");
    refused_at(&source, "@@reserved(40000)", &["outside the ordinal range"]);
}

#[test]
fn r4_reserved_needs_an_ordinal_enum() {
    let source = payment_with("  Pending\n  @@reserved(4)");
    refused_at(
        &source,
        "@@reserved(4)",
        &["no variant of enum `PaymentStatus` has one"],
    );
}

#[test]
fn malformed_lines_are_refused_where_they_are() {
    let source = payment_with("  Pending = one");
    refused_at(
        &source,
        "Pending = one",
        &["must be an integer", "`one` is not one"],
    );
    let source = payment_with("  = 1");
    refused_at(&source, "= 1", &["single identifier per line"]);
    let source = payment_with("  Pending Review = 1");
    refused_at(
        &source,
        "Pending Review = 1",
        &["single identifier per line"],
    );
    let source = payment_with("  Pending = 1\n  @@reserved()");
    refused_at(&source, "@@reserved()", &["`` is not one"]);
    let source = payment_with("  Pending = 1\n  @@reserved(4)\n  @@reserved(6)");
    refused_at(&source, "@@reserved(6)", &["at most one `@@reserved(..)`"]);
}

#[test]
fn no_other_attribute_is_read_as_a_variant() {
    // Before ordinals, `@@map("x")` with no whitespace became a variant
    // named `@@map("x")`.
    for line in ["@@map(\"payment_status\")", "@deprecated", "@@reserved 4"] {
        let source = payment_with(&format!("  Pending\n  {line}"));
        refused_at(
            &source,
            line,
            &["an enum accepts no attribute but `@@reserved(n, …)`"],
        );
    }
}

#[test]
fn a_trailing_comment_on_a_string_variant_is_a_comment() {
    let schema =
        parse_schema("enum Role {\n  admin // full access\n  member\n}\n").expect("parses");
    let names: Vec<_> = schema.enums[0]
        .variants
        .iter()
        .map(|v| v.name.as_str())
        .collect();
    assert_eq!(names, ["admin", "member"]);
}
