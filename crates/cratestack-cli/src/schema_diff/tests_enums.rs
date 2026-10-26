#![cfg(test)]
//! `cratestack diff` on enums (`enums.rs`, `response_reach.rs`).

use super::Severity;
use super::test_support::{categories, diff, diff_unvalidated};

/// `Role` is read back on `User`; `Channel` only ever arrives as an argument.
fn schema(role: &str, channel: &str) -> String {
    format!(
        "enum Role {{\n{role}\n}}\n\nenum Channel {{\n{channel}\n}}\n\n\
         model User {{\n  id Int @id\n  role Role\n}}\n\n\
         mutation procedure notify(channel: Channel): Boolean\n"
    )
}

const ROLE: &str = "  admin\n  member";
const CHANNEL: &str = "  email\n  sms";

#[test]
fn an_unchanged_enum_reports_nothing() {
    let base = schema(ROLE, CHANNEL);
    assert!(diff(&base, &base).changes.is_empty());
}

#[test]
fn removing_a_variant_is_breaking() {
    let result = diff(&schema(ROLE, CHANNEL), &schema("  admin", CHANNEL));
    assert_eq!(
        categories(&result, Severity::Breaking),
        ["enum_variant_removed"]
    );
    assert!(
        result.changes[0].message.contains("`member`"),
        "{:?}",
        result.changes
    );
}

#[test]
fn a_variant_added_to_an_enum_responses_carry_is_breaking() {
    let result = diff(
        &schema(ROLE, CHANNEL),
        &schema("  admin\n  member\n  guest", CHANNEL),
    );
    assert_eq!(
        categories(&result, Severity::Breaking),
        ["enum_variant_added"]
    );
    assert!(result.changes[0].message.contains("refuses the new value"));
}

#[test]
fn a_variant_added_to_a_request_only_enum_is_additive() {
    let result = diff(
        &schema(ROLE, CHANNEL),
        &schema(ROLE, "  email\n  sms\n  push"),
    );
    assert!(!result.has_breaking(), "{:?}", result.changes);
    assert_eq!(
        categories(&result, Severity::Additive),
        ["enum_variant_added"]
    );
}

#[test]
fn an_enum_reached_through_a_returned_type_counts_as_a_response() {
    let with = |variants: &str| {
        format!(
            "enum Tier {{\n{variants}\n}}\n\ntype Plan {{\n  tier Tier\n}}\n\n\
             procedure plans(): Plan[]\n"
        )
    };
    let result = diff(&with("  free\n  pro"), &with("  free\n  pro\n  team"));
    assert_eq!(
        categories(&result, Severity::Breaking),
        ["enum_variant_added"]
    );
}

#[test]
fn reordering_a_string_enum_is_internal() {
    let result = diff(
        &schema(ROLE, CHANNEL),
        &schema("  member\n  admin", CHANNEL),
    );
    assert_eq!(
        categories(&result, Severity::Internal),
        ["enum_variants_reordered"]
    );
    assert!(!result.has_breaking());
}

#[test]
fn removing_and_adding_an_enum() {
    let without = "model User {\n  id Int @id\n}\n";
    let with = format!("{without}\nenum Mood {{\n  calm\n}}\n");
    assert_eq!(
        categories(&diff(&with, without), Severity::Breaking),
        ["enum_removed"]
    );
    assert_eq!(
        categories(&diff(without, &with), Severity::Additive),
        ["enum_added"]
    );
}

#[test]
fn an_ordinal_enum_is_matched_by_number() {
    let before = schema("  admin = 1\n  member = 2", CHANNEL);
    // A rename keeps ordinal 2 on the wire but changes client identifiers.
    let renamed = diff_unvalidated(&before, &schema("  admin = 1\n  user = 2", CHANNEL));
    assert_eq!(
        categories(&renamed, Severity::Breaking),
        ["enum_variant_renamed"]
    );
    // Reordering lines is nothing at all: each value names itself.
    let reordered = diff_unvalidated(&before, &schema("  member = 2\n  admin = 1", CHANNEL));
    assert!(reordered.changes.is_empty(), "{:?}", reordered.changes);
    // A renumbering drops the old value and adds a new one.
    let renumbered = diff_unvalidated(&before, &schema("  admin = 1\n  member = 3", CHANNEL));
    assert_eq!(
        categories(&renumbered, Severity::Breaking),
        ["enum_variant_removed", "enum_variant_added"]
    );
}

#[test]
fn switching_between_names_and_ordinals_is_breaking() {
    let names = schema(ROLE, CHANNEL);
    let ordinals = schema("  admin = 1\n  member = 2", CHANNEL);
    for (from, to) in [(&names, &ordinals), (&ordinals, &names)] {
        let result = diff_unvalidated(from, to);
        assert_eq!(
            categories(&result, Severity::Breaking),
            ["enum_encoding_changed"]
        );
    }
}
