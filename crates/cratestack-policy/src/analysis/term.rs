//! Classifying one policy term: does it name a role, and can it hold for a
//! caller with no principal?
//!
//! Every verdict here restates what the evaluator does with an anonymous
//! `CratestackContext` (`predicate.rs` for procedures; the SQL lowering of
//! `ReadPredicate` for models, where an absent claim binds NULL and
//! satisfies neither `=` nor `<>`):
//!
//! | term | holds with no principal |
//! |---|---|
//! | `true` | yes |
//! | `false` | no (never holds) |
//! | `auth() == null` | yes |
//! | `auth() != null`, `auth().isSystem()`, `hasRole(..)`, `inTenant(..)` | no |
//! | a comparison with an `auth().<claim>` on either side | no: an absent claim satisfies neither `==` nor `!=` |
//! | `auth() == <relation>` (model) | no |
//! | any other comparison, `in [..]` list or bare boolean field | yes: it reads the input or the row, not the caller |
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
    Known {
        holds_anonymously: bool,
        role: Option<String>,
    },
    /// Not understood: reported, and treated as anonymous (fail closed).
    Unanalysed,
}

fn known(holds_anonymously: bool) -> Term {
    Term::Known {
        holds_anonymously,
        role: None,
    }
}

pub(super) fn classify_term(term: &str) -> Term {
    match term {
        AUTH_NOT_NULL_TERM | "false" => return known(false),
        AUTH_IS_NULL_TERM | "true" => return known(true),
        _ => {}
    }
    if is_auth_is_system_term(term) {
        return known(false);
    }
    if let Some(call) = parse_builtin_policy_call(term) {
        return match call {
            Ok(("hasRole", role)) => Term::Known {
                holds_anonymously: false,
                role: Some(role.to_owned()),
            },
            Ok(("inTenant", _)) => known(false),
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
        return known(true);
    }
    Term::Unanalysed
}

fn classify_comparison(lhs: &str, rhs: &str) -> Term {
    if lhs.is_empty() || rhs.is_empty() || lhs == "null" || rhs == "null" {
        return Term::Unanalysed;
    }
    let on_auth = |side: &str| side.starts_with("auth()");
    known(!(on_auth(lhs) || on_auth(rhs)))
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
