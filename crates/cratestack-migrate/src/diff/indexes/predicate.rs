//! Canonicalizes a `where:` predicate for comparison — never for DDL
//! (`AddIndex::where_predicate` is always carried through and rendered
//! verbatim, see `emit::postgres::indexes`/`emit::sqlite::indexes`).
//! [`normalize_predicate`] is the sole entry point `super::predicates_match`
//! calls; [`casts::strip_literal_casts`] does the cast-specific half of the
//! work and lives in its own file purely to keep each file under this
//! crate's ~200-LoC convention — read `normalize_predicate`'s doc first,
//! it explains *why* each step exists.

mod casts;

/// Best-effort canonicalization used only to decide whether a `where:`
/// predicate changed.
///
/// Exists because the two sides being compared come from different
/// sources: the `next`-side predicate is the `.cstack` author's literal
/// text, while the `prev`-side predicate (when it came from live
/// introspection rather than a prior schema snapshot) is Postgres's own
/// `pg_get_expr(indpred, indrelid)` deparse of the *stored* predicate,
/// which is always normalized. Verified empirically against a live
/// Postgres 18 (cratestack#742's verification evidence) rather than
/// assumed. Three independent normalizations were observed, and this
/// function reproduces two of them:
///
/// 1. **Whitespace collapse** — internal whitespace is folded to single
///    spaces.
/// 2. **A single wrapping pair of parentheses** around the whole
///    expression — e.g. `idempotency_key IS NOT NULL` round-trips as
///    `(idempotency_key IS NOT NULL)`.
/// 3. **An explicit `::type` cast inserted onto every literal** compared
///    against a column — e.g. `status = 'active'` round-trips as
///    `(status = 'active'::text)`, and `amount > 100` against a
///    `Decimal`/`NUMERIC` column round-trips as
///    `(amount > (100)::numeric)`. Left unhandled (as this function did
///    before cratestack#742's post-review remediation), this normalization
///    alone means *any* predicate containing a literal comparison never
///    compares equal to its introspected form, and the index is dropped
///    and recreated on every single `migrate` run — the ticket's
///    load-bearing "no churn" requirement, unmet. [`casts::strip_literal_casts`]
///    reproduces this one: it is a targeted tokenizer over quoted-string
///    and numeric literals immediately followed by `::<type>` (optionally
///    wrapped in one redundant pair of parens, as Postgres does for
///    numeric constants), not a general SQL parser — casts on anything
///    other than a bare literal (a column reference, a function call) are
///    intentionally left alone.
///
/// It deliberately does **not** attempt Postgres's remaining
/// normalization — identifier/keyword case-folding — since reproducing
/// that correctly needs a real SQL expression parser, which
/// cratestack#742 explicitly scopes out ("Parsing or validating the
/// predicate expression"). A predicate that differs from the stored form
/// only by identifier case (e.g. `Status = 'x'` vs. `status = 'x'`) will
/// still be reported as "changed" and get dropped/recreated on the next
/// plan — a known, documented limitation, not a silent gap (pinned by
/// `tests::case_folded_identifiers_still_do_not_normalize_equal`):
/// writing the predicate the way Postgres would already deparse it
/// (lowercase identifiers, or matching whatever the previous
/// introspection already normalized to) avoids the churn.
pub(super) fn normalize_predicate(predicate: &str) -> String {
    let collapsed: String = predicate.split_whitespace().collect::<Vec<_>>().join(" ");
    let cast_stripped = casts::strip_literal_casts(&collapsed);
    let mut inner = cast_stripped.as_str();
    loop {
        let stripped = strip_matching_outer_parens(inner);
        if stripped == inner {
            break;
        }
        inner = stripped;
    }
    format!("({inner})")
}

/// Strips one pair of parentheses from `value` iff its first `(` and
/// last `)` actually match each other — i.e. the whole string is one
/// parenthesized group, not e.g. `(a) AND (b)`, whose first `(` closes
/// before the string ends. Returns `value` unchanged when it isn't
/// wrapped that way (including when it has no surrounding parens at
/// all), so the caller's loop terminates.
fn strip_matching_outer_parens(value: &str) -> &str {
    if !value.starts_with('(') || !value.ends_with(')') {
        return value;
    }
    let mut depth = 0i32;
    let last_index = value.len() - 1;
    for (index, ch) in value.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 && index != last_index {
                    return value;
                }
            }
            _ => {}
        }
    }
    &value[1..last_index]
}
