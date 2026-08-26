//! Decides whether a `where:` predicate changed for comparison purposes
//! only — never for DDL (`AddIndex::where_predicate` is always carried
//! through and rendered verbatim, see `emit::postgres::indexes`/
//! `emit::sqlite::indexes`). [`predicates_equivalent`] is the sole entry
//! point `super::predicates_match` calls; [`casts`] does the literal/cast
//! tokenizing and lives in its own file purely to keep each file under
//! this crate's ~200-LoC convention.
//!
//! Exists because the two sides being compared come from different
//! sources: the `next`-side predicate is the `.cstack` author's literal
//! text, while the `prev`-side predicate (when it came from live
//! introspection rather than a prior schema snapshot) is Postgres's own
//! `pg_get_expr(indpred, indrelid)` deparse of the *stored* predicate,
//! which is always normalized. Verified empirically against a live
//! Postgres 18 (cratestack#742's verification evidence) rather than
//! assumed. Three independent normalizations were observed:
//!
//! 1. **Whitespace collapse** — internal whitespace is folded to single
//!    spaces. Handled unconditionally below.
//! 2. **A single wrapping pair of parentheses** around the whole
//!    expression — e.g. `idempotency_key IS NOT NULL` round-trips as
//!    `(idempotency_key IS NOT NULL)`. Handled unconditionally below.
//! 3. **An explicit `::type` cast inserted onto every literal** compared
//!    against a column — e.g. `status = 'active'` round-trips as
//!    `(status = 'active'::text)`. Left unhandled entirely (as of
//!    #742's initial landing), this alone means *any* predicate
//!    containing a literal comparison never compares equal to its
//!    introspected form, and the index is dropped and recreated on
//!    every single `migrate` run — the ticket's load-bearing "no churn"
//!    requirement, unmet.
//!
//! [`predicates_equivalent`] does **not** handle (3) by independently
//! stripping casts from both sides before a plain string comparison —
//! that was tried first (#742's initial post-review remediation) and is
//! actively unsafe: it throws the type name away entirely, so two
//! predicates that cast the *same* literal to two *genuinely different*
//! types compare equal. `amount > '100'::int` and `amount > '100'::text`
//! collapse to the identical string; the money-relevant case is
//! `email = 'x'::citext` (case-insensitive uniqueness) vs.
//! `email = 'x'::text` (case-sensitive) — an author changing the cast
//! got **no migration**, silently keeping the database enforcing the old
//! rule. That is the wrong failure direction: an unnecessary
//! drop+recreate is a noticed annoyance, a missed one is a wrong
//! uniqueness rule nobody notices until it lets a duplicate through.
//!
//! Instead, [`casts::tokenize`] splits each side into literal-vs-other
//! [`casts::Segment`]s, and [`predicates_equivalent`] compares them
//! pairwise: two `Literal` segments at the same position match if their
//! literal text is identical AND either side lacks an explicit cast
//! (forgiven — presumed `pg_get_expr`-inserted) OR both sides have one
//! and the (normalized) type names agree. Two *different* explicit
//! casts on the same literal — Finding A's failure mode — now compare
//! as **not** equivalent, forcing a drop+recreate. A structural mismatch
//! (different segment counts, a `Literal` lined up against an `Other`)
//! also fails toward **not** equivalent — churn, never silent equality.
//!
//! It deliberately does **not** attempt Postgres's remaining
//! normalization — identifier/keyword case-folding — since reproducing
//! that correctly needs a real SQL expression parser, which
//! cratestack#742 explicitly scopes out ("Parsing or validating the
//! predicate expression"). A predicate that differs from the stored form
//! only by identifier case (e.g. `Status = 'x'` vs. `status = 'x'`) will
//! still be reported as "changed" and get dropped/recreated on the next
//! plan — a known, documented limitation, not a silent gap (pinned by
//! `tests::case_folded_identifiers_still_do_not_normalize_equal`).
//! Schema-qualified vs. bare type names on an *explicit* cast
//! (`public.citext` vs. `citext`) are likewise treated as genuinely
//! different rather than guessed to be the same catalog type — there's
//! no catalog access here to confirm it, and guessing risks the exact
//! false-equality class this module exists to close (pinned by
//! `tests::schema_qualified_and_bare_type_names_do_not_match`).

mod casts;

use casts::Segment;

/// Whether two `where:` predicates should be treated as the same
/// constraint — see this module's doc for the full comparison rules.
pub(super) fn predicates_equivalent(prev: &str, next: &str) -> bool {
    let prev_segments = casts::tokenize(&canonicalize_wrapping(prev));
    let next_segments = casts::tokenize(&canonicalize_wrapping(next));
    if prev_segments.len() != next_segments.len() {
        return false;
    }
    prev_segments
        .iter()
        .zip(next_segments.iter())
        .all(|(a, b)| segments_match(a, b))
}

fn segments_match(a: &Segment, b: &Segment) -> bool {
    match (a, b) {
        (Segment::Other(x), Segment::Other(y)) => x == y,
        (
            Segment::Literal {
                text: text_a,
                cast_type: cast_a,
            },
            Segment::Literal {
                text: text_b,
                cast_type: cast_b,
            },
        ) => {
            if text_a != text_b {
                return false;
            }
            match (cast_a, cast_b) {
                // One side lacking a cast is forgiven — presumed to be
                // Postgres's own insertion the author didn't write, or a
                // cast Postgres didn't need to store at all.
                (None, _) | (_, None) => true,
                (Some(type_a), Some(type_b)) => type_a == type_b,
            }
        }
        // A literal lined up against non-literal text is a structural
        // difference, not a normalization artifact.
        _ => false,
    }
}

/// Whitespace-collapses `predicate`, then canonicalizes it to have
/// exactly one wrapping pair of parentheses around the whole expression
/// — regardless of whether the input had zero or one — since Postgres's
/// own deparse always adds exactly one and a hand-written schema
/// predicate usually has none.
fn canonicalize_wrapping(predicate: &str) -> String {
    let collapsed: String = predicate.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut inner = collapsed.as_str();
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
