use super::predicate::normalize_predicate;

#[test]
fn wraps_a_bare_predicate_in_one_pair_of_parens() {
    assert_eq!(
        normalize_predicate("idempotency_key IS NOT NULL"),
        "(idempotency_key IS NOT NULL)"
    );
}

#[test]
fn matches_postgres_own_normalized_form() {
    // Observed verbatim from `pg_get_expr` against a live Postgres
    // 18 for `CREATE UNIQUE INDEX ... WHERE idempotency_key IS NOT
    // NULL` (cratestack#742 verification evidence).
    assert_eq!(
        normalize_predicate("idempotency_key IS NOT NULL"),
        normalize_predicate("(idempotency_key IS NOT NULL)")
    );
}

#[test]
fn collapses_irregular_whitespace() {
    assert_eq!(
        normalize_predicate("  idempotency_key   IS  NOT  NULL  "),
        "(idempotency_key IS NOT NULL)"
    );
}

#[test]
fn does_not_merge_two_separately_parenthesized_clauses_into_one() {
    // `(a) AND (b)` must not be mistaken for a single wrapped
    // group — stripping the outer chars naively would produce the
    // syntactically broken `a) AND (b`.
    assert_eq!(
        normalize_predicate("(amount > 100) AND (note = 'x')"),
        "((amount > 100) AND (note = 'x'))"
    );
}

#[test]
fn strips_genuinely_redundant_nested_parens() {
    assert_eq!(
        normalize_predicate("((idempotency_key IS NOT NULL))"),
        "(idempotency_key IS NOT NULL)"
    );
}

#[test]
fn a_changed_predicate_does_not_normalize_equal() {
    assert_ne!(
        normalize_predicate("idempotency_key IS NOT NULL"),
        normalize_predicate("idempotency_key IS NULL")
    );
}

// --- cratestack#742 post-review remediation (Finding 1): the churn bug
// itself — a literal comparison gets an explicit `::type` cast on
// introspection, and the pre-remediation `normalize_predicate` didn't
// strip it, so these pairs never compared equal. Each pair below is a
// verbatim (schema text, introspected text) pair for the shapes the
// finding named as a minimum bar.

#[test]
fn strips_a_simple_text_cast() {
    // `status = 'active'` introspects as `(status = 'active'::text)`.
    assert_eq!(
        normalize_predicate("status = 'active'"),
        normalize_predicate("(status = 'active'::text)")
    );
}

#[test]
fn strips_a_text_cast_with_no_surrounding_parens() {
    assert_eq!(
        normalize_predicate("note = 'x'"),
        normalize_predicate("note = 'x'::text")
    );
}

#[test]
fn strips_a_multi_word_type_name_cast() {
    assert_eq!(
        normalize_predicate("note = 'x'"),
        normalize_predicate("note = 'x'::character varying")
    );
}

#[test]
fn strips_a_parenthesized_numeric_cast() {
    // `amount > 100` against a `Decimal`/`NUMERIC` column introspects as
    // `(amount > (100)::numeric)` — Postgres wraps the numeric constant
    // in a redundant pair of parens before the cast.
    assert_eq!(
        normalize_predicate("amount > 100"),
        normalize_predicate("(amount > (100)::numeric)")
    );
}

#[test]
fn strips_a_parenthesized_integer_cast() {
    assert_eq!(
        normalize_predicate("amount > 1"),
        normalize_predicate("(amount > (1)::integer)")
    );
}

#[test]
fn strips_multiple_casts_in_the_same_predicate() {
    assert_eq!(
        normalize_predicate("(amount > 100) AND (note = 'x')"),
        normalize_predicate("((amount > (100)::numeric) AND (note = 'x'::text))")
    );
}

#[test]
fn does_not_strip_a_cast_on_a_column_reference() {
    // Out of scope per the finding: casts on anything other than a bare
    // literal (a column, a function call) are intentionally left alone,
    // since telling a column reference from a literal in general needs a
    // real SQL parser.
    assert_ne!(
        normalize_predicate("status = 'active'"),
        normalize_predicate("status::text = 'active'::text")
    );
}

#[test]
fn a_genuinely_different_numeric_literal_still_compares_unequal_after_cast_stripping() {
    assert_ne!(
        normalize_predicate("(amount > (100)::numeric)"),
        normalize_predicate("(amount > (200)::numeric)")
    );
}

/// Documents the one normalization this function still doesn't attempt
/// (identifier/keyword case-folding) — see `normalize_predicate`'s doc
/// for why. Pinned so a future change to this scope decision is a
/// visible, deliberate diff here, not a silent behavior change.
#[test]
fn case_folded_identifiers_still_do_not_normalize_equal() {
    assert_ne!(
        normalize_predicate("Status = 'active'"),
        normalize_predicate("status = 'active'")
    );
}
