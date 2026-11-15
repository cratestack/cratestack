//! What a schema's `@allow`/`@deny` rules say about who may call an op,
//! computed from the compiler's own parsing (`cratestack contract ops`,
//! cratestack#1388).
//!
//! The question a consumer asks is "which roles does this op name, and can
//! it be called with no principal?" (an API gateway's route table, a relay's
//! per-app allowlist). Answering it by hand means porting the policy grammar
//! and its quirks, which drifts. This module answers it over the same
//! [`PolicyAst`](crate::PolicyAst) and term recognisers the macros lower
//! from; [`term`] states what each term means for an anonymous caller.
//!
//! # `requires_principal`
//!
//! Every policy node evaluates, for an anonymous caller, to
//! [`AlwaysFalse`](term::Anon::AlwaysFalse) or
//! [`MaybeTrue`](term::Anon::MaybeTrue) ([`term`] has the leaf table and the
//! relation-quantifier rule). `&&` is `AlwaysFalse` if any child is; `||`
//! only if all of them are. Several `@allow` lines are alternatives, so
//! `requires_principal` is `true` exactly when every `@allow` is
//! `AlwaysFalse`, or when there is no `@allow` (the evaluator denies
//! everyone: the empty `allow` list is how a consumer tells that apart from
//! a real principal requirement). `@deny` is not consulted: it can only
//! refuse, so it cannot make an op callable.
//!
//! It is never `true` for an op an anonymous caller can pass: `none` and
//! `every` over an `auth()` comparison are `MaybeTrue` (`NOT EXISTS (.. AND
//! FALSE)` is true for every row, and `every` is vacuously true with no
//! related rows), and anything not understood is `MaybeTrue` too.
//!
//! `roles` is every role *named* anywhere in an `@allow`, including under a
//! `!=`-style negation or a branch that cannot hold; it is a superset of the
//! roles that grant access, never the grant itself.
//!
//! A rule the analysis does not understand (a malformed expression, an
//! unknown function, a term shape with no row in [`term`]) is listed in
//! `unanalysed`, and then `requires_principal` is `false`, so a consumer
//! that cares must refuse an op with a non-empty `unanalysed` rather than
//! allow-list it.

mod term;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_literals;
#[cfg(test)]
mod tests_quantifiers;
#[cfg(test)]
mod tests_runtime_agreement;

use std::collections::BTreeSet;

use crate::syntax::{PolicyAst, parse_policy_ast};
use term::{Anon, Term, classify_term};

pub use term::PolicyDialect;

/// The verdict for one op. See the [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpPolicyAnalysis {
    /// Every `hasRole("…")` literal *named* anywhere in an `@allow`, sorted
    /// and deduplicated: a superset of the roles that grant access. A `@deny`
    /// adds none.
    pub roles: Vec<String>,
    /// `true` when no `@allow` rule can hold with no principal.
    pub requires_principal: bool,
    /// Rule texts (allow or deny) the analysis did not fully understand.
    pub unanalysed: Vec<String>,
}

/// Analyse the expressions of an op's (written in `dialect`) `@allow` and `@deny` rules (the text
/// inside the parentheses, as the macros see it).
pub fn analyse_op_policy(
    dialect: PolicyDialect,
    allow: &[&str],
    deny: &[&str],
) -> OpPolicyAnalysis {
    let mut roles = BTreeSet::new();
    let mut unanalysed = Vec::new();
    let mut any_allow_anonymous = false;
    let mut allow_unanalysed = false;
    for (is_allow, rule) in allow
        .iter()
        .map(|rule| (true, *rule))
        .chain(deny.iter().map(|rule| (false, *rule)))
    {
        let mut found = Found {
            dialect,
            roles: BTreeSet::new(),
            holds_anonymously: false,
        };
        let understood = parse_policy_ast(rule)
            .map(|ast| found.walk(&ast))
            .is_ok_and(|(_, understood)| understood);
        if !understood {
            unanalysed.push(rule.to_owned());
        }
        if is_allow {
            roles.extend(found.roles);
            allow_unanalysed |= !understood;
            any_allow_anonymous |= found.holds_anonymously;
        }
    }
    OpPolicyAnalysis {
        roles: roles.into_iter().collect(),
        requires_principal: !(any_allow_anonymous || allow_unanalysed),
        unanalysed,
    }
}

struct Found {
    dialect: PolicyDialect,
    roles: BTreeSet<String>,
    holds_anonymously: bool,
}

impl Found {
    /// `(can hold anonymously, every term understood)` of `ast`; collects
    /// the roles on the way and records the root verdict.
    fn walk(&mut self, ast: &PolicyAst) -> (bool, bool) {
        let (anon, understood) = self.node(ast);
        self.holds_anonymously = anon == Anon::MaybeTrue;
        (self.holds_anonymously, understood)
    }

    fn node(&mut self, ast: &PolicyAst) -> (Anon, bool) {
        match ast {
            PolicyAst::Term(text) => match classify_term(text, self.dialect) {
                Term::Known { anon, role } => {
                    self.roles.extend(role);
                    (anon, true)
                }
                Term::Unanalysed => (Anon::MaybeTrue, false),
            },
            PolicyAst::And(parts) => self.combine(parts, true),
            PolicyAst::Or(parts) => self.combine(parts, false),
            // A node kind added later: not understood, so `MaybeTrue`.
            #[allow(unreachable_patterns)]
            _ => (Anon::MaybeTrue, false),
        }
    }

    /// `&&` is `AlwaysFalse` if any child is; `||` only if all are.
    fn combine(&mut self, parts: &[PolicyAst], all: bool) -> (Anon, bool) {
        let mut any_false = false;
        let mut all_false = true;
        let mut understood = true;
        for part in parts {
            let (anon, part_understood) = self.node(part);
            let is_false = anon == Anon::AlwaysFalse;
            any_false |= is_false;
            all_false &= is_false;
            understood &= part_understood;
        }
        let always_false = if all { any_false } else { all_false };
        let anon = if always_false {
            Anon::AlwaysFalse
        } else {
            Anon::MaybeTrue
        };
        (anon, understood)
    }
}
