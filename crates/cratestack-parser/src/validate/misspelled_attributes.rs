//! Near-miss rejection for misspelled field attributes (cratestack#679's
//! typo half).
//!
//! `.cstack` attributes parse generically into an opaque `Attribute { raw,
//! span }`, and an unrecognised one is simply inert. That is how a typo'd
//! `@raedonly` used to report `schema OK` while quietly leaving a field
//! ordinary and writable — the schema author gets positive confirmation
//! that a protection is in place when it is not. #679 calls this out as
//! failing in the unsafe direction, and it is the half
//! `crate::validate::removed_attributes` deliberately left open (see that
//! module's doc).
//!
//! # Why near-miss, and not a closed attribute set
//!
//! Maintainer decision on #679, choosing option (b) over option (a).
//!
//! Option (a) — reject *any* attribute not on an allowlist — matches the
//! ticket's first acceptance criterion literally, but commits the language
//! to a closed set and carries real blast radius: the supported set has to
//! be reconstructed from scattered `raw == "@…"` comparisons and validator
//! match arms with no in-repo spec to derive it from, it must be correct
//! for all five field-bearing declaration kinds (`model`, `view`, `mixin`,
//! `type`, `auth`), and a too-narrow list breaks every affected user's
//! schema on upgrade. That is a worse failure than the silent no-op it
//! replaces.
//!
//! Option (b), implemented here: an unknown attribute is rejected **only**
//! when it is a near-miss of a name the language knows. So `@raedonly`
//! fails and names `@readonly`, while `@totallyBogusAttribute` stays inert
//! exactly as before. This covers the case #679's Summary actually argues
//! about — a typo silently dropping an intended protection — at a fraction
//! of the risk.
//!
//! # Why a generous reference set is the safe direction
//!
//! [`KNOWN_ATTRIBUTE_NAMES`] deliberately lists every attribute name the
//! language knows *at any position*, not just the ones valid on a field.
//! Under option (b) that asymmetry is safe in the right direction:
//!
//! - An **extra** name only ever *reduces* detections (it makes an input
//!   "known", so this module stays silent) — it can never cause a false
//!   rejection.
//! - A **missing** name is the only real hazard: a genuinely supported
//!   attribute absent from the list could be flagged as a near-miss of
//!   some other listed name.
//!
//! So the list errs toward inclusion. It also deliberately contains the
//! names `crate::validate::removed_attributes` rejects outright (`@allow`,
//! `@deny`, `@pb`, `@custom`): including them makes this module silent on
//! an exact use, leaving that module's specific, more useful guidance to
//! fire instead — while a *typo* of one still gets pointed at the real
//! spelling, which then draws the real explanation.
//!
//! This is **not** a claim that every listed name is valid on a field.
//! Suggesting a procedure-position attribute for a field typo is a
//! slightly imprecise hint, not a wrong rejection: it still names the
//! spelling the author meant, which is the whole job here.

use cratestack_core::Field;

use crate::diagnostics::{SchemaError, span_error};

/// Every attribute name the `.cstack` language knows, at any declaration
/// position. See this module's doc for why this is deliberately generous
/// rather than field-scoped.
///
/// Derived from the union of (1) every `raw == "@…"` / `starts_with("@…")`
/// comparison across the workspace, (2) `crate::validate::validators`'
/// match arms, (3) `crate::validate::removed_attributes`'
/// `REJECTED_FIELD_ATTRIBUTES`, and (4) every single-`@` name appearing in
/// the repo's committed `.cstack` files. Sources (1) and (4) are both
/// necessary and neither is sufficient: `@uri` is accepted by
/// `validators.rs` but appears in no schema, while `@from` and
/// `@authorize` appear in schemas and in neither of the first two sets.
const KNOWN_ATTRIBUTE_NAMES: &[&str] = &[
    "allow",
    "api_version",
    "authorize",
    "computed",
    "custom",
    "db_enforce",
    "default",
    "deny",
    "deprecated",
    "email",
    "from",
    "id",
    "iso4217",
    "isolation",
    "length",
    "no_idempotency",
    "no_rate_limit",
    "pb",
    "pii",
    "range",
    "readonly",
    "regex",
    "relation",
    "sensitive",
    "server_only",
    "status",
    "stream",
    "unique",
    "uri",
    "use",
    "version",
];

/// Inputs shorter than this are never considered for a suggestion.
///
/// At one or two characters, almost anything is within edit distance 1 of
/// something (`@ix` would "mean" `@id`), so the suggestion stops being
/// evidence of a typo and starts being noise. Every real attribute this
/// short (`@id`, `@use`, `@pb`) is in [`KNOWN_ATTRIBUTE_NAMES`] and so
/// never reaches this path anyway.
const MIN_LENGTH_FOR_SUGGESTION: usize = 3;

/// The bare name of an attribute, with any `(...)` argument list and the
/// leading `@` stripped: `@length(min: 1)` -> `length`.
fn bare_name(raw: &str) -> &str {
    let without_sigil = raw.strip_prefix('@').unwrap_or(raw);
    match without_sigil.find('(') {
        Some(open) => &without_sigil[..open],
        None => without_sigil,
    }
}

/// How far apart two names may be before a suggestion stops being
/// credible. Scaled by length so a short name needs a closer match: two
/// edits on a six-character name is a plausible typo, two edits on a
/// four-character one is usually a different word.
fn max_distance_for(name: &str) -> usize {
    if name.chars().count() <= 5 { 1 } else { 2 }
}

/// Optimal string alignment distance — Levenshtein plus adjacent
/// transposition as a single edit.
///
/// The transposition case is load-bearing rather than a refinement:
/// `raedonly` -> `readonly` is a plain transposition, which costs 2 under
/// Levenshtein but 1 here. #679's own worked example is exactly that
/// shape, so without transposition support the canonical case would need
/// the looser distance-2 threshold and drag in far more noise with it.
fn optimal_string_alignment(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut distances = vec![vec![0usize; right.len() + 1]; left.len() + 1];

    for (row, entry) in distances.iter_mut().enumerate() {
        entry[0] = row;
    }
    for (column, entry) in distances[0].iter_mut().enumerate() {
        *entry = column;
    }

    for row in 1..=left.len() {
        for column in 1..=right.len() {
            let substitution_cost = usize::from(left[row - 1] != right[column - 1]);
            let mut best = (distances[row - 1][column] + 1)
                .min(distances[row][column - 1] + 1)
                .min(distances[row - 1][column - 1] + substitution_cost);
            if row > 1
                && column > 1
                && left[row - 1] == right[column - 2]
                && left[row - 2] == right[column - 1]
            {
                best = best.min(distances[row - 2][column - 2] + 1);
            }
            distances[row][column] = best;
        }
    }
    distances[left.len()][right.len()]
}

/// The closest known attribute name to `name`, if one is close enough to
/// be worth suggesting.
///
/// Comparison is case-insensitive so a pure case error (`@ReadOnly`)
/// surfaces too — that reaches here only when the exact, case-sensitive
/// membership test has already failed, so a distance of 0 means the name
/// differs *only* by case and is unambiguously a typo.
fn closest_known_name(name: &str) -> Option<&'static str> {
    if name.chars().count() < MIN_LENGTH_FOR_SUGGESTION {
        return None;
    }
    let lowered = name.to_ascii_lowercase();
    let limit = max_distance_for(&lowered);
    KNOWN_ATTRIBUTE_NAMES
        .iter()
        .map(|known| (*known, optimal_string_alignment(&lowered, known)))
        .filter(|(_, distance)| *distance <= limit)
        .min_by_key(|(known, distance)| (*distance, known.len()))
        .map(|(known, _)| known)
}

/// Rejects a field attribute that is a near-miss of a known attribute
/// name.
///
/// Runs *after* [`crate::validate::removed_attributes`] at every call
/// site, so an exact `@allow`/`@deny`/`@pb`/`@custom` gets that module's
/// specific guidance rather than this one's generic suggestion. (Those
/// names are in [`KNOWN_ATTRIBUTE_NAMES`], so this module is silent on
/// them regardless — the ordering is belt-and-braces, and documented so a
/// future reader does not "simplify" it by removing one of the two.)
///
/// An unknown attribute that is *not* a near-miss is left inert, which is
/// the pre-existing behaviour and the deliberate limit of option (b).
pub(super) fn validate_misspelled_field_attributes(
    owner_kind: &str,
    owner_name: &str,
    field: &Field,
) -> Result<(), SchemaError> {
    for attribute in &field.attributes {
        let name = bare_name(&attribute.raw);
        if KNOWN_ATTRIBUTE_NAMES.contains(&name) {
            continue;
        }
        let Some(suggestion) = closest_known_name(name) else {
            continue;
        };
        return Err(span_error(
            format!(
                "field `{}` on {} `{}` uses unknown attribute `@{}` — did you mean `@{}`? \
                 An unrecognised attribute is inert: it parses, reports `schema OK`, and \
                 enforces nothing, so a typo here silently drops whatever the real attribute \
                 would have done (cratestack#679)",
                field.name, owner_kind, owner_name, name, suggestion,
            ),
            field.span,
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "misspelled_attributes_tests.rs"]
mod misspelled_attributes_tests;
