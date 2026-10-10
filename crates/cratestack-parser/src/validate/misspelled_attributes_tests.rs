//! Unit coverage for the distance/suggestion machinery (cratestack#679).
//!
//! The user-facing behaviour (a misspelled or unknown attribute fails
//! `parse_schema` on all five field-bearing declarations) is asserted in
//! `crate::tests_field_attrs` and `crate::tests_field_attribute_lists`, so
//! those go through the real parser and the real message. What lives here is
//! the part that is awkward to reach that way: the exact boundary of what
//! counts as "close enough", which is where a "did you mean" either earns its
//! keep or turns into noise.

use super::{closest_name, max_distance_for, optimal_string_alignment};
use crate::validate::field_attributes::suggestion_pool;

/// The canonical #679 case, and the reason transposition is handled as a
/// single edit rather than two substitutions.
#[test]
fn a_transposition_is_one_edit() {
    assert_eq!(optimal_string_alignment("raedonly", "readonly"), 1);
    assert_eq!(optimal_string_alignment("readonly", "readonly"), 0);
}

#[test]
fn the_ticket_typo_suggests_the_attribute_it_meant() {
    assert_eq!(
        closest_name("@raedonly", suggestion_pool().iter().copied()),
        Some("@readonly")
    );
}

/// What used to be left inert (#679's option (b)) is refused by the closed
/// list whether or not it resembles anything; it just gets no suggestion.
#[test]
fn a_name_that_resembles_nothing_gets_no_suggestion() {
    let pool = suggestion_pool();
    for written in [
        "@totallyBogusAttribute",
        "@whatever",
        "@string",
        "@wire",
        "@bigint",
        "@immutable",
    ] {
        assert_eq!(
            closest_name(written, pool.iter().copied()),
            None,
            "{written}"
        );
    }
}

/// Guards the noise floor. At one or two characters nearly everything is
/// one edit from something, so a suggestion would stop being evidence of
/// a typo.
#[test]
fn very_short_names_never_produce_a_suggestion() {
    let pool = suggestion_pool();
    assert_eq!(closest_name("@ix", pool.iter().copied()), None);
    assert_eq!(closest_name("@q", pool.iter().copied()), None);
}

/// A pure case or sigil difference reaches this path only after the exact,
/// case-sensitive membership test has already failed, so distance 0 means
/// the name differs *only* by case or sigil: unambiguously a typo.
#[test]
fn a_case_or_sigil_only_difference_is_suggested() {
    let pool = suggestion_pool();
    assert_eq!(
        closest_name("@ReadOnly", pool.iter().copied()),
        Some("@readonly")
    );
    assert_eq!(
        closest_name("@SERVER_ONLY", pool.iter().copied()),
        Some("@server_only")
    );
    assert_eq!(
        closest_name("@@readonly", pool.iter().copied()),
        Some("@readonly")
    );
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
/// primary key, unlike `@raedonly` which used to fail silently.
#[test]
fn the_length_floor_takes_precedence_over_case_only_detection() {
    assert_eq!(closest_name("@Id", suggestion_pool().iter().copied()), None);
}

/// A typo of a removed name is pointed at it, which then draws that
/// attribute's own explanation (`super::removed_attributes`).
#[test]
fn a_typo_of_a_removed_attribute_is_pointed_at_it() {
    let pool = suggestion_pool();
    assert_eq!(closest_name("@alow", pool.iter().copied()), Some("@allow"));
    assert_eq!(closest_name("@dney", pool.iter().copied()), Some("@deny"));
}

/// The pool is every name some kind accepts plus the removed ones, each
/// found as itself.
#[test]
fn every_pool_name_is_found_as_itself() {
    let pool = suggestion_pool();
    for name in pool {
        // `@id` and `@pb` are under the length floor and never suggested.
        let expected = (name.trim_start_matches('@').len() >= 3).then_some(*name);
        assert_eq!(closest_name(name, pool.iter().copied()), expected, "{name}");
    }
}

/// No name in the pool may be a near-miss of another. If two were, a typo
/// of one could be "corrected" to the other, and it would mean the language
/// has two attributes a user can confuse by a single edit.
///
/// This currently passes; it is here to fail the moment a *new* attribute
/// is added that is one edit from an existing one, which is a naming
/// decision worth making deliberately rather than discovering through a
/// confusing suggestion.
#[test]
fn no_two_pool_names_are_within_suggestion_distance() {
    let pool = suggestion_pool();
    for (index, left) in pool.iter().enumerate() {
        for right in &pool[index + 1..] {
            let (left_bare, right_bare) =
                (left.trim_start_matches('@'), right.trim_start_matches('@'));
            let distance = optimal_string_alignment(left_bare, right_bare);
            let limit = max_distance_for(left_bare).max(max_distance_for(right_bare));
            assert!(
                distance > limit,
                "`{left}` and `{right}` are only {distance} edit(s) apart, within the \
                 suggestion threshold of {limit}: a typo of one would be suggested as the \
                 other. Rename one, or reconsider the threshold."
            );
        }
    }
}
