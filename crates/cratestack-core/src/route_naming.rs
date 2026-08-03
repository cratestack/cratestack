//! Canonical REST route-segment derivation for a model name.
//!
//! cratestack#345: three independently-implemented algorithms (the
//! server's real Axum route registration in
//! `cratestack-macros::axum::model::routes`, and near-identical
//! `split_words`-based reimplementations in
//! `cratestack-client-typescript::naming` and
//! `cratestack-client-dart::idents`) all computed what is supposed to be
//! the same wire contract — a model's REST route path — and drifted apart
//! for model names containing a literal `_` (legal today; no parser
//! grammar restriction). `to_snake_case`/`pluralize` here are exactly the
//! server's original algorithm, byte-for-byte, moved into this crate
//! (already a shared dependency of `cratestack-macros`,
//! `cratestack-client-typescript`, and `cratestack-client-dart`) so every
//! consumer imports the same implementation instead of reimplementing it.
//!
//! **Do not reimplement these.** Any code deriving a model's REST route
//! must call [`model_route_segment`] (or `to_snake_case` + `pluralize`
//! directly, in that order) from here.

/// Converts `value` to snake_case by inserting a single `_` before every
/// non-initial uppercase character. Every other character — including any
/// pre-existing `_` — passes through unchanged.
///
/// This is intentionally *not* a natural-language word tokenizer: it does
/// not treat `_`/`-`/` ` as separators to drop and rejoin. A model named
/// `User_Group` therefore becomes `user__group` (double underscore), not
/// `user_group` — that looks odd, but it is what the server's route
/// registration has always produced, and every client generator must
/// match it exactly to avoid constructing a URL the server never
/// registers.
pub fn to_snake_case(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_uppercase() {
            if index > 0 {
                output.push('_');
            }
            for lowercase in character.to_lowercase() {
                output.push(lowercase);
            }
        } else {
            output.push(character);
        }
    }
    output
}

/// Naive English pluralization matching the server's route registration:
/// values ending in `s` get `es` appended, everything else gets a bare
/// `s`. Deliberately not grammatically complete (e.g. `category` becomes
/// `categorys`, not `categories`) — it only needs to match the server,
/// not be linguistically ideal.
pub fn pluralize(value: &str) -> String {
    if value.ends_with('s') {
        format!("{value}es")
    } else {
        format!("{value}s")
    }
}

/// The canonical REST route segment (no leading `/`) for a model name —
/// `pluralize(&to_snake_case(model_name))`. The server's Axum route
/// registration and every client generator's route derivation must
/// produce this exact string for a given model name; call this instead of
/// composing `to_snake_case`/`pluralize` locally so the composition order
/// can't drift either.
pub fn model_route_segment(model_name: &str) -> String {
    pluralize(&to_snake_case(model_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// cratestack#345's own repro table, plus additional tricky cases:
    /// consecutive-uppercase runs (acronym-like names), a trailing
    /// underscore, hyphen/space boundaries (not legal in `.cstack` model
    /// names today, but the functions themselves must still behave
    /// correctly for any input string), and combinations thereof.
    const CASES: &[(&str, &str, &str)] = &[
        ("UserGroup", "user_group", "user_groups"),
        ("User_Group", "user__group", "user__groups"),
        ("User_group", "user_group", "user_groups"),
        ("HTTPServer", "h_t_t_p_server", "h_t_t_p_servers"),
        ("User_", "user_", "user_s"),
        ("_User", "__user", "__users"),
        ("User__Group", "user___group", "user___groups"),
        ("User-Group", "user-_group", "user-_groups"),
        ("User Group", "user _group", "user _groups"),
        ("Bus", "bus", "buses"),
        ("Class", "class", "classes"),
        ("Category", "category", "categorys"),
        ("already_snake", "already_snake", "already_snakes"),
    ];

    #[test]
    fn to_snake_case_matches_table() {
        for (input, expected_snake, _) in CASES {
            assert_eq!(
                to_snake_case(input),
                *expected_snake,
                "to_snake_case({input:?})"
            );
        }
    }

    #[test]
    fn pluralize_matches_table() {
        for (_, snake, expected_plural) in CASES {
            assert_eq!(pluralize(snake), *expected_plural, "pluralize({snake:?})");
        }
    }

    #[test]
    fn model_route_segment_composes_snake_then_pluralize() {
        for (input, _, expected_plural) in CASES {
            assert_eq!(
                model_route_segment(input),
                *expected_plural,
                "model_route_segment({input:?})"
            );
        }
    }

    /// The exact mismatch cratestack#345 reports: before the fix, the
    /// server's algorithm and the TS/Dart `split_words`-based algorithm
    /// disagreed for `User_Group`. Pin the server-ground-truth value here
    /// so a regression can't silently swap it back to the old
    /// word-tokenizing behavior.
    #[test]
    fn user_group_with_underscore_is_not_the_naive_tokenized_form() {
        // The old `split_words`-based client algorithm treated `_` as a
        // separator and would have produced "user_groups" here — the
        // same route as plain `UserGroup`, a guaranteed server 404 and a
        // route collision between two distinct model names.
        assert_ne!(model_route_segment("User_Group"), "user_groups");
        assert_eq!(model_route_segment("User_Group"), "user__groups");
    }
}
