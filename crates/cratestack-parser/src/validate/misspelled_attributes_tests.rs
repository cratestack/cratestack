//! Unit coverage for the distance/suggestion machinery (cratestack#679).
//!
//! The user-facing behaviour — that a typo'd attribute fails
//! `parse_schema` on all five field-bearing declarations, and that a
//! non-near-miss still parses — is asserted in `crate::tests_field_attrs`
//! alongside the `removed_attributes` cases, so those go through the real
//! parser and the real message. What lives here is the part that is
//! awkward to reach that way: the exact boundary of what counts as "close
//! enough", which is where option (b) either earns its keep or turns into
//! noise.

use super::{KNOWN_ATTRIBUTE_NAMES, bare_name, closest_known_name, optimal_string_alignment};

#[test]
fn bare_name_strips_the_sigil_and_any_argument_list() {
    assert_eq!(bare_name("@readonly"), "readonly");
    assert_eq!(bare_name("@length(min: 1, max: 200)"), "length");
    assert_eq!(
        bare_name("@relation(fields: [a], references: [b])"),
        "relation"
    );
    // Defensive: the parser always includes the sigil, but this must not
    // panic or mangle the name if that ever changes.
    assert_eq!(bare_name("readonly"), "readonly");
}

/// The canonical #679 case, and the reason transposition is handled as a
/// single edit rather than two substitutions.
#[test]
fn a_transposition_is_one_edit() {
    assert_eq!(optimal_string_alignment("raedonly", "readonly"), 1);
    assert_eq!(optimal_string_alignment("readonly", "readonly"), 0);
}

#[test]
fn the_ticket_typo_suggests_the_attribute_it_meant() {
    assert_eq!(closest_known_name("raedonly"), Some("readonly"));
}

/// The other half of option (b), and the reason it was chosen over a
/// closed attribute set: an attribute that is not a near-miss of anything
/// stays inert rather than becoming a parse error. #679's
/// `@totallyBogusAttribute` is the ticket's own example.
#[test]
fn a_name_that_resembles_nothing_is_left_alone() {
    assert_eq!(closest_known_name("totallyBogusAttribute"), None);
    assert_eq!(closest_known_name("whatever"), None);
}

/// Guards the noise floor. At one or two characters nearly everything is
/// one edit from something, so a suggestion would stop being evidence of
/// a typo.
#[test]
fn very_short_names_never_produce_a_suggestion() {
    assert_eq!(closest_known_name("ix"), None);
    assert_eq!(closest_known_name("q"), None);
}

/// A pure case difference reaches this path only after the exact,
/// case-sensitive membership test has already failed — so distance 0 here
/// means the name differs *only* by case, which is unambiguously a typo.
#[test]
fn a_case_only_difference_is_suggested() {
    assert_eq!(closest_known_name("ReadOnly"), Some("readonly"));
    assert_eq!(closest_known_name("SERVER_ONLY"), Some("server_only"));
}

/// The length floor wins over case-insensitivity, and that ordering is
/// deliberate rather than incidental: `@Id` differs from `@id` only by
/// case and is *still* not suggested, because at two characters the floor
/// rejects it before any distance is computed.
///
/// Asserted rather than left implicit because the two rules pull in
/// opposite directions here, and a future reader tempted to "fix" the
/// case-only path for short names would be reintroducing the noise the
/// floor exists to prevent. `@Id` is also not a real hazard: it is one
/// keystroke from valid and produces an immediately visible missing
/// primary key, unlike `@raedonly` which fails silently.
#[test]
fn the_length_floor_takes_precedence_over_case_only_detection() {
    assert_eq!(closest_known_name("Id"), None);
}

/// Every name in the reference set must be recognised exactly, or the
/// module would suggest an attribute in place of itself.
#[test]
fn every_known_name_is_zero_distance_from_itself() {
    for known in KNOWN_ATTRIBUTE_NAMES {
        assert_eq!(
            optimal_string_alignment(known, known),
            0,
            "{known} should be identical to itself"
        );
    }
}

/// The set is used as a membership test *and* as a suggestion source, so
/// a duplicate would be silently harmless but a sign the list was edited
/// carelessly — and this list is exactly the thing whose completeness the
/// module's safety argument rests on.
#[test]
fn the_known_set_has_no_duplicates() {
    let mut sorted = KNOWN_ATTRIBUTE_NAMES.to_vec();
    sorted.sort_unstable();
    let mut deduped = sorted.clone();
    deduped.dedup();
    assert_eq!(
        sorted, deduped,
        "KNOWN_ATTRIBUTE_NAMES contains a duplicate"
    );
}

/// No known name may be a near-miss of another. If two were, a typo of
/// one could be "corrected" to the other, and — worse — it would mean the
/// language has two attributes a user can confuse by a single edit.
///
/// This currently passes; it is here to fail the moment a *new* attribute
/// is added that is one edit from an existing one, which is a naming
/// decision worth making deliberately rather than discovering through a
/// confusing suggestion.
#[test]
fn no_two_known_names_are_within_suggestion_distance() {
    for (index, left) in KNOWN_ATTRIBUTE_NAMES.iter().enumerate() {
        for right in &KNOWN_ATTRIBUTE_NAMES[index + 1..] {
            let distance = optimal_string_alignment(left, right);
            let limit = super::max_distance_for(left).max(super::max_distance_for(right));
            assert!(
                distance > limit,
                "`@{left}` and `@{right}` are only {distance} edit(s) apart, within the \
                 suggestion threshold of {limit} — a typo of one would be suggested as the \
                 other. Rename one, or reconsider the threshold."
            );
        }
    }
}
