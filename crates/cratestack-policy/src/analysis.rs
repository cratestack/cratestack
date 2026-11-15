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
//! `true` when no `@allow` rule can hold for a caller with no principal.
//! Each rule is an expression tree: `||` can hold anonymously when *any*
//! branch can, `&&` only when *every* branch can. Several `@allow` lines are
//! alternatives, so the op needs a principal only when every line does.
//! `@deny` is not consulted: it can only refuse, so it cannot make an op
//! callable, and whether it fires depends on the input. With no `@allow` at
//! all the evaluator denies everyone (`authorize_procedure`), so no
//! anonymous caller passes and the verdict is `true`; the empty `allow` list
//! is how a consumer tells that apart from a real principal requirement.
//!
//! A rule the analysis does not understand (a malformed expression, an
//! unknown function, a term shape with no row in [`term`]) is listed in
//! `unanalysed`, and then `requires_principal` is `false`: fail closed, in
//! the sense that the op is treated as anonymous so a consumer that cares
//! must refuse it rather than silently allow-list it as protected.

mod term;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_runtime_agreement;

use std::collections::BTreeSet;

use crate::syntax::{PolicyAst, parse_policy_ast};
use term::{Term, classify_term};

/// The verdict for one op. See the [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpPolicyAnalysis {
    /// Every `hasRole("…")` literal reachable in an `@allow`, sorted and
    /// deduplicated. A `@deny` adds none.
    pub roles: Vec<String>,
    /// `true` when no `@allow` rule can hold with no principal.
    pub requires_principal: bool,
    /// Rule texts (allow or deny) the analysis did not fully understand.
    pub unanalysed: Vec<String>,
}

/// Analyse the expressions of an op's `@allow` and `@deny` rules (the text
/// inside the parentheses, as the macros see it).
pub fn analyse_op_policy(allow: &[&str], deny: &[&str]) -> OpPolicyAnalysis {
    let mut roles = BTreeSet::new();
    let mut unanalysed = Vec::new();
    let mut any_allow_anonymous = false;
    let mut allow_unanalysed = false;
    for (is_allow, rule) in allow
        .iter()
        .map(|rule| (true, *rule))
        .chain(deny.iter().map(|rule| (false, *rule)))
    {
        let mut found = Found::default();
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

#[derive(Default)]
struct Found {
    roles: BTreeSet<String>,
    holds_anonymously: bool,
}

impl Found {
    /// `(can hold anonymously, every term understood)` of `ast`; collects
    /// the roles on the way and records the root verdict.
    fn walk(&mut self, ast: &PolicyAst) -> (bool, bool) {
        let verdict = self.node(ast);
        self.holds_anonymously = verdict.0;
        verdict
    }

    fn node(&mut self, ast: &PolicyAst) -> (bool, bool) {
        match ast {
            PolicyAst::Term(text) => match classify_term(text) {
                Term::Known {
                    holds_anonymously,
                    role,
                } => {
                    self.roles.extend(role);
                    (holds_anonymously, true)
                }
                Term::Unanalysed => (true, false),
            },
            PolicyAst::And(parts) => self.combine(parts, true),
            PolicyAst::Or(parts) => self.combine(parts, false),
        }
    }

    fn combine(&mut self, parts: &[PolicyAst], all: bool) -> (bool, bool) {
        let mut holds = all;
        let mut understood = true;
        for part in parts {
            let (part_holds, part_understood) = self.node(part);
            holds = if all {
                holds && part_holds
            } else {
                holds || part_holds
            };
            understood &= part_understood;
        }
        (holds, understood)
    }
}
