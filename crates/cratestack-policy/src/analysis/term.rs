//! Classifying one policy term: does it name a role, and what is it worth to
//! a caller with no principal?
//!
//! Each term evaluates, for an anonymous `CratestackContext`, to an
//! [`Anon`]: [`Anon::AlwaysFalse`] (no row and no input can make it hold) or
//! [`Anon::MaybeTrue`] (it can). The verdicts restate the evaluator
//! (`predicate.rs` for procedures; the SQL lowering of `ReadPredicate` for
//! models, where an absent claim renders `FALSE`):
//!
//! | term | anonymous |
//! |---|---|
//! | `true` | `MaybeTrue` |
//! | `false` | `AlwaysFalse` |
//! | `auth() == null` | `MaybeTrue` |
//! | `auth() != null`, `auth().isSystem()`, `hasRole(..)`, `inTenant(..)` | `AlwaysFalse` |
//! | a comparison with an `auth()` side (`auth().x == ..`, `col == auth().x`, `auth() == <relation>`) | `AlwaysFalse`, unless a `none`/`every` quantifier wraps it (below) |
//! | any other comparison, `in [..]` list or bare boolean field | `MaybeTrue`: it reads the input or the row, not the caller |
//!
//! # Relation quantifiers
//!
//! A model path through a to-many relation names its quantifier
//! (`memberships.none.userId == auth().id`). The `AlwaysFalse` leaf is
//! rendered inside it, and the quantifier decides what the whole is:
//!
//! - `some` and a to-one hop: `EXISTS (.. AND FALSE)` is false, so the
//!   verdict is the leaf's.
//! - `none`: `NOT EXISTS (.. AND FALSE)` is true for every row.
//! - `every`: `NOT EXISTS (.. AND NOT (FALSE))` is true for every row with no
//!   related rows (vacuous truth).
//!
//! So a `none` or `every` anywhere in the path makes the term `MaybeTrue`
//! however deep the `auth()` leaf sits. Where the analysis is unsure it says
//! `MaybeTrue`: that is the direction that never reports a principal as
//! required for an op an anonymous caller can pass.
//!
//! The recognisers are the macros' own ([`crate::syntax`]), applied in the
//! macros' order; a term they would reject, or one this table has no row
//! for, is [`Term::Unanalysed`].

use crate::syntax::{
    AUTH_IS_NULL_TERM, AUTH_NOT_NULL_TERM, is_auth_is_system_term, parse_builtin_policy_call,
};

/// What one term means for the caller it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Term {
    /// Understood. `role` is the `hasRole` literal it names, if any.
    Known { anon: Anon, role: Option<String> },
    /// Not understood: reported, and treated as `MaybeTrue`.
    Unanalysed,
}

/// What a policy node is worth to a caller with no principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Anon {
    /// Holds for no row and no input.
    AlwaysFalse,
    /// Can hold; also the answer for anything not understood.
    MaybeTrue,
}

fn known(anon: Anon) -> Term {
    Term::Known { anon, role: None }
}

pub(super) fn classify_term(term: &str) -> Term {
    match term {
        AUTH_NOT_NULL_TERM | "false" => return known(Anon::AlwaysFalse),
        AUTH_IS_NULL_TERM | "true" => return known(Anon::MaybeTrue),
        _ => {}
    }
    if is_auth_is_system_term(term) {
        return known(Anon::AlwaysFalse);
    }
    if let Some(call) = parse_builtin_policy_call(term) {
        return match call {
            Ok(("hasRole", role)) => Term::Known {
                anon: Anon::AlwaysFalse,
                role: Some(role.to_owned()),
            },
            Ok(("inTenant", _)) => known(Anon::AlwaysFalse),
            _ => Term::Unanalysed,
        };
    }
    if let Some((lhs, rhs)) = term.split_once("==").or_else(|| term.split_once("!=")) {
        return classify_comparison(lhs.trim(), rhs.trim());
    }
    if term.contains("auth()") {
        return Term::Unanalysed;
    }
    if is_membership(term) || is_path(term) {
        return known(Anon::MaybeTrue);
    }
    Term::Unanalysed
}

fn classify_comparison(lhs: &str, rhs: &str) -> Term {
    if lhs.is_empty() || rhs.is_empty() || lhs == "null" || rhs == "null" {
        return Term::Unanalysed;
    }
    let on_auth = |side: &str| side.starts_with("auth()");
    if !(on_auth(lhs) || on_auth(rhs)) {
        return known(Anon::MaybeTrue);
    }
    // The `AlwaysFalse` leaf, possibly inside relation quantifiers on the
    // other side's path: `none` and `every` turn it true (module docs).
    let negates_leaf = [lhs, rhs]
        .into_iter()
        .filter(|side| !on_auth(side))
        .flat_map(|side| side.split('.'))
        .any(|segment| matches!(segment.trim(), "none" | "every"));
    known(if negates_leaf {
        Anon::MaybeTrue
    } else {
        Anon::AlwaysFalse
    })
}

/// `field in [A, B]` / `field not in [A, B]` (cratestack#666).
fn is_membership(term: &str) -> bool {
    let Some((field, list)) = term
        .split_once(" in [")
        .or_else(|| term.split_once(" not in ["))
    else {
        return false;
    };
    let field = field.trim();
    list.ends_with(']') && !field.is_empty() && is_path(field.trim_end_matches(" not"))
}

/// A bare boolean field or relation path: `published`, `author.active`.
fn is_path(term: &str) -> bool {
    let mut chars = term.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && term
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}
