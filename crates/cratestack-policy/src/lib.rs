use cratestack_core::{CoolContext, CoolError, Value};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationQuantifier {
    ToOne,
    Some,
    Every,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyLiteral {
    Bool(bool),
    Int(i64),
    String(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadPredicate {
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
        value: PolicyLiteral,
    },
    AuthFieldNeLiteral {
        auth_field: &'static str,
        value: PolicyLiteral,
    },
    FieldIsTrue {
        column: &'static str,
    },
    FieldEqLiteral {
        column: &'static str,
        value: PolicyLiteral,
    },
    FieldNeLiteral {
        column: &'static str,
        value: PolicyLiteral,
    },
    FieldEqAuth {
        column: &'static str,
        auth_field: &'static str,
    },
    FieldNeAuth {
        column: &'static str,
        auth_field: &'static str,
    },
    Relation {
        quantifier: RelationQuantifier,
        parent_table: &'static str,
        parent_column: &'static str,
        related_table: &'static str,
        related_column: &'static str,
        expr: &'static PolicyExpr,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadPolicy {
    pub expr: PolicyExpr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyExpr {
    Predicate(ReadPredicate),
    And(&'static [PolicyExpr]),
    Or(&'static [PolicyExpr]),
}

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

pub fn authorize_procedure<A: ProcedureArgs + ?Sized>(
    allow_policies: &[ProcedurePolicy],
    deny_policies: &[ProcedurePolicy],
    args: &A,
    ctx: &CoolContext,
) -> Result<(), CoolError> {
    if allow_policies.is_empty() {
        return Err(CoolError::Forbidden(
            "procedure policy denied this operation".to_owned(),
        ));
    }

    if deny_policies
        .iter()
        .any(|policy| procedure_policy_expr_matches(policy.expr, args, ctx))
    {
        return Err(CoolError::Forbidden(
            "procedure policy denied this operation".to_owned(),
        ));
    }

    if allow_policies
        .iter()
        .any(|policy| procedure_policy_expr_matches(policy.expr, args, ctx))
    {
        Ok(())
    } else {
        Err(CoolError::Forbidden(
            "procedure policy denied this operation".to_owned(),
        ))
    }
}

pub fn context_has_role(ctx: &CoolContext, role: &str) -> bool {
    ctx.auth_field("role")
        .or_else(|| ctx.auth_field("actor.role"))
        .is_some_and(|value| matches!(value, Value::String(candidate) if candidate == role))
}

pub fn context_in_tenant(ctx: &CoolContext, tenant_id: &str) -> bool {
    ctx.auth_field("tenant.id")
        .is_some_and(|value| matches!(value, Value::String(candidate) if candidate == tenant_id))
}

fn procedure_policy_expr_matches<A: ProcedureArgs + ?Sized>(
    expr: ProcedurePolicyExpr,
    args: &A,
    ctx: &CoolContext,
) -> bool {
    match expr {
        ProcedurePolicyExpr::Predicate(predicate) => {
            procedure_predicate_matches(predicate, args, ctx)
        }
        ProcedurePolicyExpr::And(exprs) => exprs
            .iter()
            .copied()
            .all(|expr| procedure_policy_expr_matches(expr, args, ctx)),
        ProcedurePolicyExpr::Or(exprs) => exprs
            .iter()
            .copied()
            .any(|expr| procedure_policy_expr_matches(expr, args, ctx)),
    }
}

fn procedure_predicate_matches<A: ProcedureArgs + ?Sized>(
    predicate: ProcedurePredicate,
    args: &A,
    ctx: &CoolContext,
) -> bool {
    match predicate {
        ProcedurePredicate::AuthNotNull => ctx.is_authenticated(),
        ProcedurePredicate::AuthIsNull => !ctx.is_authenticated(),
        ProcedurePredicate::HasRole { role } => context_has_role(ctx, role),
        ProcedurePredicate::InTenant { tenant_id } => context_in_tenant(ctx, tenant_id),
        ProcedurePredicate::AuthFieldEqLiteral { auth_field, value } => ctx
            .auth_field(auth_field)
            .is_some_and(|candidate| value_matches_literal(candidate, value)),
        ProcedurePredicate::AuthFieldNeLiteral { auth_field, value } => ctx
            .auth_field(auth_field)
            .is_some_and(|candidate| !value_matches_literal(candidate, value)),
        ProcedurePredicate::InputFieldIsTrue { field } => args
            .procedure_arg_value(field)
            .is_some_and(|value| value == Value::Bool(true)),
        ProcedurePredicate::InputFieldEqLiteral { field, value } => args
            .procedure_arg_value(field)
            .is_some_and(|candidate| value_matches_literal(&candidate, value)),
        ProcedurePredicate::InputFieldNeLiteral { field, value } => args
            .procedure_arg_value(field)
            .is_some_and(|candidate| !value_matches_literal(&candidate, value)),
        ProcedurePredicate::InputFieldEqAuth { field, auth_field } => {
            match (args.procedure_arg_value(field), ctx.auth_field(auth_field)) {
                (Some(left), Some(right)) => &left == right,
                _ => false,
            }
        }
        ProcedurePredicate::InputFieldNeAuth { field, auth_field } => {
            match (args.procedure_arg_value(field), ctx.auth_field(auth_field)) {
                (Some(left), Some(right)) => &left != right,
                _ => false,
            }
        }
        ProcedurePredicate::InputFieldEqInput { field, other_field } => {
            match (
                args.procedure_arg_value(field),
                args.procedure_arg_value(other_field),
            ) {
                (Some(left), Some(right)) => left == right,
                _ => false,
            }
        }
        ProcedurePredicate::InputFieldNeInput { field, other_field } => {
            match (
                args.procedure_arg_value(field),
                args.procedure_arg_value(other_field),
            ) {
                (Some(left), Some(right)) => left != right,
                _ => false,
            }
        }
    }
}

fn value_matches_literal(value: &Value, literal: ProcedurePolicyLiteral) -> bool {
    match (value, literal) {
        (Value::Bool(left), ProcedurePolicyLiteral::Bool(right)) => *left == right,
        (Value::Int(left), ProcedurePolicyLiteral::Int(right)) => *left == right,
        (Value::String(left), ProcedurePolicyLiteral::String(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn has_role_checks_top_level_and_actor_role() {
        let top_level =
            CoolContext::authenticated([("role".to_owned(), Value::String("admin".to_owned()))]);
        assert!(context_has_role(&top_level, "admin"));
        assert!(!context_has_role(&top_level, "member"));

        let actor_role = CoolContext::authenticated([(
            "actor".to_owned(),
            Value::Map(BTreeMap::from([(
                "role".to_owned(),
                Value::String("merchant".to_owned()),
            )])),
        )]);
        assert!(context_has_role(&actor_role, "merchant"));
    }

    #[test]
    fn in_tenant_checks_structured_tenant_id() {
        let ctx = CoolContext::authenticated([(
            "tenant".to_owned(),
            Value::Map(BTreeMap::from([(
                "id".to_owned(),
                Value::String("tenant_1".to_owned()),
            )])),
        )]);
        assert!(context_in_tenant(&ctx, "tenant_1"));
        assert!(!context_in_tenant(&ctx, "tenant_2"));
    }
}
