//! Procedure-side policy types and the `ProcedureArgs` trait.

use cratestack_core::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcedurePolicyLiteral {
    Bool(bool),
    Int(i64),
    String(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcedurePredicate {
    /// A bare boolean literal used as a whole `@allow`/`@deny` clause, e.g.
    /// `@allow(true)`. Unlike every other variant, this reads nothing off
    /// `args`/`ctx` — the outcome is fixed at schema-compile time. Exists
    /// so `@allow(true)`/`@allow(false)` parse as the literal predicates
    /// they read as, rather than as an unresolved input-field reference
    /// named `true`/`false`.
    Literal(bool),
    AuthNotNull,
    AuthIsNull,
    /// Lowered from `auth().isSystem()` (issue cratestack#486). Satisfied
    /// only by a `CratestackContext` minted through
    /// `cratestack_core::SystemContext`.
    ///
    /// The read-path twin (`ReadPredicate::AuthIsSystem`) shipped with
    /// cratestack#486; this one did not, and its absence was only found
    /// when cratestack#867 tried to write the reconciliation query that
    /// motivated the whole feature. Same fail-closed property: it can
    /// only make a policy `TRUE` where a clause names it, so a procedure
    /// or query whose policies never mention it is entirely unaffected by
    /// system callers.
    AuthIsSystem,
    HasRole {
        role: &'static str,
    },
    InTenant {
        tenant_id: &'static str,
    },
    AuthFieldEqLiteral {
        auth_field: &'static str,
        value: ProcedurePolicyLiteral,
    },
    AuthFieldNeLiteral {
        auth_field: &'static str,
        value: ProcedurePolicyLiteral,
    },
    InputFieldIsTrue {
        field: &'static str,
    },
    InputFieldEqLiteral {
        field: &'static str,
        value: ProcedurePolicyLiteral,
    },
    InputFieldNeLiteral {
        field: &'static str,
        value: ProcedurePolicyLiteral,
    },
    InputFieldEqAuth {
        field: &'static str,
        auth_field: &'static str,
    },
    InputFieldNeAuth {
        field: &'static str,
        auth_field: &'static str,
    },
    InputFieldEqInput {
        field: &'static str,
        other_field: &'static str,
    },
    InputFieldNeInput {
        field: &'static str,
        other_field: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcedurePolicy {
    pub expr: ProcedurePolicyExpr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcedurePolicyExpr {
    Predicate(ProcedurePredicate),
    And(&'static [ProcedurePolicyExpr]),
    Or(&'static [ProcedurePolicyExpr]),
}

pub trait ProcedureArgs {
    fn procedure_arg_value(&self, field: &str) -> Option<Value>;
}

impl ProcedureArgs for () {
    fn procedure_arg_value(&self, _field: &str) -> Option<Value> {
        None
    }
}
