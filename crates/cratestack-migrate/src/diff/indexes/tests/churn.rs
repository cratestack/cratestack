//! Round 1 (cratestack#742): the basic churn-tolerance behavior —
//! whitespace/paren normalization, and one side bearing a `::type` cast
//! the other lacks being forgiven.

use super::super::predicate::predicates_equivalent;

#[test]
fn a_bare_predicate_matches_itself() {
    assert!(predicates_equivalent(
        "idempotency_key IS NOT NULL",
        "idempotency_key IS NOT NULL"
    ));
}

#[test]
fn matches_postgres_own_normalized_form() {
    // Observed verbatim from `pg_get_expr` against a live Postgres
    // 18 for `CREATE UNIQUE INDEX ... WHERE idempotency_key IS NOT
    // NULL` (cratestack#742 verification evidence).
    assert!(predicates_equivalent(
        "idempotency_key IS NOT NULL",
        "(idempotency_key IS NOT NULL)"
    ));
}

#[test]
fn collapses_irregular_whitespace() {
    assert!(predicates_equivalent(
        "  idempotency_key   IS  NOT  NULL  ",
        "idempotency_key IS NOT NULL"
    ));
}

#[test]
fn does_not_merge_two_separately_parenthesized_clauses_into_one() {
    // `(a) AND (b)` must not be mistaken for a single wrapped
    // group — stripping the outer chars naively would produce the
    // syntactically broken `a) AND (b`. Still matches itself.
    assert!(predicates_equivalent(
        "(amount > 100) AND (note = 'x')",
        "(amount > 100) AND (note = 'x')"
    ));
}

#[test]
fn strips_genuinely_redundant_nested_parens() {
    assert!(predicates_equivalent(
        "((idempotency_key IS NOT NULL))",
        "idempotency_key IS NOT NULL"
    ));
}

#[test]
fn a_changed_predicate_does_not_match() {
    assert!(!predicates_equivalent(
        "idempotency_key IS NOT NULL",
        "idempotency_key IS NULL"
    ));
}

#[test]
fn tolerates_a_simple_text_cast_the_other_side_lacks() {
    // `status = 'active'` introspects as `(status = 'active'::text)`.
    assert!(predicates_equivalent(
        "status = 'active'",
        "(status = 'active'::text)"
    ));
}

#[test]
fn tolerates_a_text_cast_with_no_surrounding_parens() {
    assert!(predicates_equivalent("note = 'x'", "note = 'x'::text"));
}

#[test]
fn tolerates_a_multi_word_type_name_cast() {
    assert!(predicates_equivalent(
        "note = 'x'",
        "note = 'x'::character varying"
    ));
}

#[test]
fn tolerates_a_parenthesized_numeric_cast() {
    // `amount > 100` against a floating-point column introspects as
    // `(amount > (100)::double precision)` — Postgres wraps the numeric
    // constant in a redundant pair of parens before the cast.
    assert!(predicates_equivalent(
        "amount > 100",
        "(amount > (100)::double precision)"
    ));
}

#[test]
fn tolerates_a_parenthesized_integer_cast() {
    assert!(predicates_equivalent(
        "amount > 1",
        "(amount > (1)::integer)"
    ));
}

#[test]
fn tolerates_multiple_tolerated_casts_in_the_same_predicate() {
    assert!(predicates_equivalent(
        "(amount > 100) AND (note = 'x')",
        "((amount > (100)::numeric) AND (note = 'x'::text))"
    ));
}

#[test]
fn does_not_tolerate_a_cast_on_a_column_reference() {
    // Out of scope per the finding: casts on anything other than a bare
    // literal (a column, a function call) are intentionally left alone,
    // since telling a column reference from a literal in general needs a
    // real SQL parser — `status::text` becomes an opaque `Other` run
    // that must match byte-for-byte, so it doesn't accidentally line up
    // with a differently-shaped `status`.
    assert!(!predicates_equivalent(
        "status = 'active'",
        "status::text = 'active'::text"
    ));
}

#[test]
fn a_genuinely_different_numeric_literal_still_does_not_match_after_cast_tolerance() {
    assert!(!predicates_equivalent(
        "(amount > (100)::numeric)",
        "(amount > (200)::numeric)"
    ));
}

/// Documents the one normalization this comparison still doesn't attempt
/// (identifier/keyword case-folding) — see `predicate`'s module doc for
/// why. Pinned so a future change to this scope decision is a visible,
/// deliberate diff here, not a silent behavior change.
#[test]
fn case_folded_identifiers_still_do_not_match() {
    assert!(!predicates_equivalent(
        "Status = 'active'",
        "status = 'active'"
    ));
}
