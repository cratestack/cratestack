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
//! The recognisers are the macros' own ([`crate::syntax`]), in the macros'
//! order and with their quirks (membership before comparison; a comparison
//! divides at the first `==`/`!=`, a literal included). A term they would
//! reject, or one this table has no row for, is [`Term::Unanalysed`].

use crate::syntax::{
    ModelTerm, ProcedureTerm, parse_string_literal, read_model_term, read_procedure_term,
    split_list_elements,
};

/// Which policy dialect a term is written in: a model's `@@allow`/`@@deny`
/// or a procedure's/query's `@allow`/`@deny`. The two split a term
/// differently (only the model dialect has `in [..]` and `auth() ==
/// <relation>`), and each is read by its own shared reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PolicyDialect {
    /// `@@allow` / `@@deny` on a model.
    Model,
    /// `@allow` / `@deny` on a procedure, query or subscription.
    Procedure,
}

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

pub(super) fn classify_term(term: &str, dialect: PolicyDialect) -> Term {
    match dialect {
        PolicyDialect::Model => classify_model_term(term),
        PolicyDialect::Procedure => classify_procedure_term(term),
    }
}

fn classify_model_term(term: &str) -> Term {
    match read_model_term(term) {
        ModelTerm::AuthNotNull | ModelTerm::AuthIsSystem => known(Anon::AlwaysFalse),
        ModelTerm::AuthIsNull => known(Anon::MaybeTrue),
        ModelTerm::Builtin(call) => classify_call(call),
        ModelTerm::AuthRelation(relation) => {
            if is_path(relation) {
                known(Anon::AlwaysFalse)
            } else {
                Term::Unanalysed
            }
        }
        ModelTerm::In { lhs, list, .. } => {
            // A list the macros refuse (empty, a trailing comma, an
            // unterminated literal) is a compile error there.
            if is_path(lhs) && split_list_elements(list, lhs).is_ok() {
                known(Anon::MaybeTrue)
            } else {
                Term::Unanalysed
            }
        }
        ModelTerm::Comparison { lhs, rhs, .. } => classify_comparison(lhs, rhs),
        ModelTerm::Bare(term) => classify_bare(term),
    }
}

fn classify_procedure_term(term: &str) -> Term {
    match read_procedure_term(term) {
        ProcedureTerm::AuthNotNull | ProcedureTerm::AuthIsSystem | ProcedureTerm::False => {
            known(Anon::AlwaysFalse)
        }
        ProcedureTerm::AuthIsNull | ProcedureTerm::True => known(Anon::MaybeTrue),
        ProcedureTerm::Builtin(call) => classify_call(call),
        ProcedureTerm::Comparison { lhs, rhs, .. } => classify_comparison(lhs, rhs),
        ProcedureTerm::Bare(term) => classify_bare(term),
    }
}

fn classify_call(call: Result<(&str, &str), String>) -> Term {
    match call {
        Ok(("hasRole", role)) => Term::Known {
            anon: Anon::AlwaysFalse,
            role: Some(role.to_owned()),
        },
        Ok(("inTenant", _)) => known(Anon::AlwaysFalse),
        _ => Term::Unanalysed,
    }
}

/// A term with no operator: a boolean field or relation path reads the row
/// or the input, not the caller. `true` and `false` are the constants; any
/// other shape (one that names `auth()`, say) is not understood.
fn classify_bare(term: &str) -> Term {
    match term {
        "false" => known(Anon::AlwaysFalse),
        "true" => known(Anon::MaybeTrue),
        _ if term.contains("auth()") => Term::Unanalysed,
        _ if is_path(term) => known(Anon::MaybeTrue),
        _ => Term::Unanalysed,
    }
}

fn classify_comparison(lhs: &str, rhs: &str) -> Term {
    let on_auth = |side: &str| side.starts_with("auth()");
    // What the macros accept: a field, relation or `auth().x` path on the
    // left, and one of those or a literal on the right. Anything else is a
    // compile error there (`x != "a == b"` leaves `x != "a` on the left), so
    // it is reported, not judged.
    let valid_side = |side: &str| is_path(side) || auth_path(side);
    let valid_rhs = valid_side(rhs)
        || parse_string_literal(rhs).is_some()
        || !rhs.contains(['"', '\'']) && !on_auth(rhs);
    if rhs.is_empty() || lhs == "null" || rhs == "null" || !valid_side(lhs) || !valid_rhs {
        return Term::Unanalysed;
    }
    if !(on_auth(lhs) || on_auth(rhs)) {
        return known(Anon::MaybeTrue);
    }
    // The `AlwaysFalse` leaf, possibly inside relation quantifiers on the
    // other side's path: `none` and `every` turn it true (module docs).
    let negates_leaf = [lhs, rhs]
        .into_iter()
        .filter(|side| !on_auth(side) && parse_string_literal(side).is_none())
        .flat_map(|side| side.split('.'))
        .any(|segment| matches!(segment.trim(), "none" | "every"));
    known(if negates_leaf {
        Anon::MaybeTrue
    } else {
        Anon::AlwaysFalse
    })
}

/// `auth().x` or `auth().x.y`, as the macros read it: prefix stripped, trimmed.
fn auth_path(side: &str) -> bool {
    side.strip_prefix("auth().")
        .is_some_and(|rest| is_path(rest.trim()))
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
