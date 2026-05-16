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
    AuthNotNull,
    AuthIsNull,
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
