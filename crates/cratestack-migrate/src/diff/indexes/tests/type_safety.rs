//! Round 2 (cratestack#742): round 1's fix independently stripped casts
//! from both sides before a plain string comparison, which discarded
//! the type name entirely — two predicates casting the SAME literal to
//! two DIFFERENT types compared as equal, silently keeping the database
//! enforcing the OLD uniqueness rule (Finding A). A second, independent
//! bug in the type-name grammar could corrupt the literal itself
//! (Finding B). These pin the fix: a joint, type-aware comparison, plus
//! every "verified safe" shape from the review so a regression here is
//! a visible, deliberate diff.

use super::super::predicate::predicates_equivalent;

#[test]
fn finding_a_different_explicit_casts_on_the_same_literal_do_not_match() {
    // The exact reproduction from the review: an author changing a
    // partial-index predicate's cast from `int` to `text` on the same
    // literal must be treated as a real change (drop + recreate), not
    // silently accepted.
    assert!(!predicates_equivalent(
        "amount > '100'::int",
        "amount > '100'::text"
    ));
}

#[test]
fn finding_a_citext_vs_text_cast_does_not_match() {
    // The money-relevant case: `citext` is case-insensitive, `text` is
    // not. A partial unique index enforcing `email = 'x'::citext` and
    // one enforcing `email = 'x'::text` are genuinely different
    // constraints and must not be conflated.
    assert!(!predicates_equivalent(
        "email = 'ADMIN@X.COM'::citext",
        "email = 'ADMIN@X.COM'::text"
    ));
}

#[test]
fn finding_a_same_explicit_cast_on_both_sides_still_matches() {
    // The joint comparison must not turn into "any two explicit casts
    // differ" — same type on both sides is still a match.
    assert!(predicates_equivalent(
        "amount > '100'::int",
        "amount > '100'::int"
    ));
}

#[test]
fn finding_b_int4_cast_does_not_corrupt_the_literal() {
    // `parse_lowercase_word`'s old ASCII-lowercase-only grammar left the
    // trailing digit of `int4` unconsumed, and it landed back on the
    // literal being compared, turning `100::int4` into `1004`. That
    // would make a hand-written `x = 1004` compare equal to
    // `x = 100::int4`, an independent false-equality vector.
    assert!(!predicates_equivalent("x = 100::int4", "x = 1004"));
    // The properly-parsed cast literal still matches itself.
    assert!(predicates_equivalent("x = 100::int4", "x = 100::int4"));
}

#[test]
fn finding_b_schema_qualified_type_name_does_not_corrupt_the_literal() {
    // The old grammar left `_catalog.int4` unconsumed after `pg` (the
    // first lowercase run), corrupting the literal to `100_catalog.int4`.
    assert!(predicates_equivalent(
        "x = 100::pg_catalog.int4",
        "x = 100::pg_catalog.int4"
    ));
}

#[test]
fn finding_b_double_quoted_type_name_does_not_corrupt_the_literal() {
    assert!(predicates_equivalent(
        "x = '1'::\"MyType\"",
        "x = '1'::\"MyType\""
    ));
    // Quoting makes the type name case-sensitive — a differently-cased
    // quoted spelling is a different type, not normalized together.
    assert!(!predicates_equivalent(
        "x = '1'::\"MyType\"",
        "x = '1'::\"mytype\""
    ));
}

#[test]
fn schema_qualified_and_bare_type_names_do_not_match() {
    // Deliberate, documented choice (`predicate`'s module doc): no
    // catalog access here to confirm `public.citext` and `citext`
    // really are the same type, so they're treated as different rather
    // than guessed to be the same — fails toward churn, never toward
    // silent equality.
    assert!(!predicates_equivalent(
        "x = '1'::public.citext",
        "x = '1'::citext"
    ));
}

#[test]
fn array_type_suffix_is_preserved_and_compared() {
    assert!(predicates_equivalent("x = 'a'::text[]", "x = 'a'::text[]"));
    assert!(!predicates_equivalent("x = 'a'::text[]", "x = 'a'::text"));
}

// --- "verified safe" shapes from the review — pinned so a future
// change can't silently regress them.

#[test]
fn string_literals_containing_double_colon_are_not_mistaken_for_a_cast() {
    assert!(predicates_equivalent("x = 'a::b'", "x = 'a::b'"));
    assert!(predicates_equivalent(
        "x = 'a::b'::text",
        "x = 'a::b'::text"
    ));
}

#[test]
fn an_escaped_quote_literal_round_trips() {
    assert!(predicates_equivalent("x = ''''::text", "x = ''''::text"));
}

#[test]
fn a_bare_double_colon_literal_is_not_mistaken_for_a_cast() {
    assert!(predicates_equivalent("x = '::'", "x = '::'"));
}

#[test]
fn an_unterminated_quote_is_left_untouched_on_both_sides() {
    assert!(predicates_equivalent(
        "x = 'unterminated",
        "x = 'unterminated"
    ));
}

#[test]
fn double_parens_around_a_numeric_cast_fail_toward_churn_not_equality() {
    // `((100))::numeric` — a genuinely ambiguous shape this module
    // doesn't try to fully unwrap — must not silently match the plain
    // `100` it would mean if fully unwrapped.
    assert!(!predicates_equivalent("100", "((100))::numeric"));
}

#[test]
fn a_double_cast_fails_toward_churn_not_equality() {
    assert!(!predicates_equivalent("100", "100::numeric::text"));
}

#[test]
fn a_double_cast_still_matches_itself() {
    assert!(predicates_equivalent(
        "100::numeric::text",
        "100::numeric::text"
    ));
}
