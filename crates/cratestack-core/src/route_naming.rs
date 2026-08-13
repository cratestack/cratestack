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

/// English pluralization used for both REST route segments and generated
/// table/column names — the single implementation `cratestack-core` and
/// `cratestack-migrate` both call (cratestack#504; previously two
/// hand-synced copies, `cratestack-migrate::naming::pluralize` mirroring
/// this one, that had already drifted apart on this exact rule).
///
/// Rules, in order: a value ending in `s` gets `es` appended (`bus` ->
/// `buses`); a value ending in a *consonant* + `y` swaps the `y` for
/// `ies` (`category` -> `categories`, `webhook_delivery` ->
/// `webhook_deliveries`); a value ending in a *vowel* + `y` (`day`) or
/// anything else just gets a bare `s` appended (`day` -> `days`).
///
/// Still not a grammatically complete English pluralizer — irregular
/// plurals (`person` -> `people`) are not handled, and there is
/// deliberately no way to override the result for a given model today.
/// `@@map(...)` is the planned escape hatch for that (flagged as a known
/// gap by cratestack#504, not yet implemented, out of scope here). This
/// function only needs to match the server's route registration and the
/// migration engine's table naming, not be linguistically ideal.
///
/// **Migrating past cratestack#504's `y -> ies` fix.** This function
/// feeds `cratestack-migrate`'s table-name derivation
/// (`cratestack_migrate::naming::table_name`), and `cratestack-migrate`'s
/// diff engine matches tables **by name only** — it never infers a
/// rename from two schemas that otherwise look related
/// (`crates/cratestack-migrate/src/diff.rs`). Any deployed model whose
/// name ends in a consonant + `y` (`Category`, `Delivery`, `Entry`,
/// `Query`, ...) changes its derived table name on this upgrade
/// (`categorys` -> `categories`). Running `cratestack migrate diff`
/// against such a schema without first declaring the rename produces
/// `DropTable(categorys)` + `CreateTable(categories)` — applying that
/// migration **destroys the table's data**.
///
/// Before running `migrate diff` after upgrading past this change, add
/// `@@rename(from = "<old_table_name>")` to every affected model (e.g.
/// `@@rename(from = "categorys")` on `model Category`) so the diff
/// engine emits `ALTER TABLE ... RENAME TO ...` instead. See
/// `crates/cratestack-migrate/src/convert/renames.rs` for the attribute
/// and `crates/cratestack-migrate/src/emit/postgres/tests/renames.rs`'s
/// `pluralization_change_with_rename_marker_is_a_rename_not_drop_and_create`
/// test for a worked example of exactly this scenario (and its sibling
/// `..._without_rename_marker_drops_and_recreates`, which pins down what
/// happens if you skip this step).
pub fn pluralize(value: &str) -> String {
    if value.ends_with('s') {
        return format!("{value}es");
    }
    if let Some(stem) = value.strip_suffix('y') {
        let preceded_by_vowel =
            matches!(stem.chars().next_back(), Some('a' | 'e' | 'i' | 'o' | 'u'));
        if !preceded_by_vowel {
            return format!("{stem}ies");
        }
    }
    format!("{value}s")
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
        // cratestack#504: consonant + `y` -> `ies`.
        ("Category", "category", "categories"),
        ("WebhookDelivery", "webhook_delivery", "webhook_deliveries"),
        ("Entry", "entry", "entries"),
        // cratestack#504: vowel + `y` -> plain `s`, not `ies`.
        ("Day", "day", "days"),
        ("Holiday", "holiday", "holidays"),
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

    /// cratestack#504's exact repro: `model WebhookDelivery` used to
    /// derive the table name `webhook_deliverys` — not just wrong-looking,
    /// but a name a hand-written migration for the grammatically correct
    /// `webhook_deliveries` would never match, breaking every query the
    /// generated model client issues against that table.
    #[test]
    fn webhook_delivery_pluralizes_to_the_grammatically_correct_form() {
        assert_eq!(model_route_segment("WebhookDelivery"), "webhook_deliveries");
        assert_ne!(model_route_segment("WebhookDelivery"), "webhook_deliverys");
    }

    /// The four rule branches in isolation, independent of the shared
    /// `CASES` table above, so each one has a test that fails for exactly
    /// one reason if the corresponding branch regresses.
    #[test]
    fn pluralize_consonant_plus_y_becomes_ies() {
        assert_eq!(pluralize("category"), "categories");
        assert_eq!(pluralize("delivery"), "deliveries");
    }

    #[test]
    fn pluralize_vowel_plus_y_becomes_plain_s() {
        assert_eq!(pluralize("day"), "days");
        assert_eq!(pluralize("key"), "keys");
    }

    #[test]
    fn pluralize_trailing_s_becomes_es() {
        assert_eq!(pluralize("bus"), "buses");
        assert_eq!(pluralize("class"), "classes");
    }

    #[test]
    fn pluralize_plain_word_gets_bare_s() {
        assert_eq!(pluralize("customer"), "customers");
        assert_eq!(pluralize("order"), "orders");
    }
}
