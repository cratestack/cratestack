//! The "did you mean" source for every closed attribute list.
//!
//! This module used to be cratestack#679's option (b): refuse a field
//! attribute only when it was a near-miss of a known name, and leave any
//! other unknown name inert. Option (a), a closed list, was set aside then
//! because there was no spec to derive the set from, it had to be right for
//! five declaration kinds, and a list too narrow breaks users on upgrade.
//! ADR 0019 D5 answers each: the per-kind tables in
//! `super::field_attribute_tables` are derived from the readers and cite
//! them, and a census of every committed schema is a test
//! (`tests/committed_schemas.rs`). A field attribute outside its kind's table is
//! now refused whether or not it resembles anything, by
//! `super::attribute_shape::check_shape` like every other closed position
//! (GHSA-69g4-xvcm-vm2j), and this module is what finds the suggestion.
//!
//! One implementation serves procedures, queries, `@@` block attributes and
//! fields, so the threshold cannot differ between them.

/// Inputs shorter than this are never considered for a suggestion.
///
/// At one or two characters, almost anything is within edit distance 1 of
/// something (`@ix` would "mean" `@id`), so the suggestion stops being
/// evidence of a typo and starts being noise.
const MIN_LENGTH_FOR_SUGGESTION: usize = 3;

/// How far apart two names may be before a suggestion stops being
/// credible. Scaled by length so a short name needs a closer match: two
/// edits on a six-character name is a plausible typo, two edits on a
/// four-character one is usually a different word.
fn max_distance_for(name: &str) -> usize {
    if name.chars().count() <= 5 { 1 } else { 2 }
}

/// Optimal string alignment distance: Levenshtein plus adjacent
/// transposition as a single edit.
///
/// The transposition case is load-bearing rather than a refinement:
/// `raedonly` -> `readonly` is a plain transposition, which costs 2 under
/// Levenshtein but 1 here. #679's own worked example is exactly that
/// shape, so without transposition support the canonical case would need
/// the looser distance-2 threshold and drag in far more noise with it.
pub(super) fn optimal_string_alignment(left: &str, right: &str) -> usize {
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

/// The name in `names` that `written` most likely means, if one is close
/// enough to be worth suggesting: the same name in another case or with
/// other sigils (`@ReadOnly`, `@@readonly`), or one a typo away.
///
/// Comparison ignores the `@` sigils and the case, so a pure case error
/// surfaces too. It reaches here only after the exact, case-sensitive
/// membership test failed, so a distance of 0 means the name differs
/// *only* by case or sigil, which is unambiguously a typo. The length floor
/// is checked first and wins over that: `@Id` is two characters, so it is
/// not suggested as `@id`.
pub(super) fn closest_name<'a>(
    written: &str,
    names: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let bare = written.trim_start_matches('@').to_ascii_lowercase();
    if bare.chars().count() < MIN_LENGTH_FOR_SUGGESTION {
        return None;
    }
    let limit = max_distance_for(&bare);
    names
        .into_iter()
        .map(|name| {
            let distance = optimal_string_alignment(&bare, name.trim_start_matches('@'));
            (name, distance)
        })
        .filter(|(_, distance)| *distance <= limit)
        .min_by_key(|(name, distance)| (*distance, name.len()))
        .map(|(name, _)| name)
}

#[cfg(test)]
#[path = "misspelled_attributes_tests.rs"]
mod misspelled_attributes_tests;
