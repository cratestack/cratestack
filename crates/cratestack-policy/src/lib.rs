//! Read- and procedure-policy types plus evaluator.

mod eval;
mod procedure_types;
mod read_types;

pub use eval::{authorize_procedure, context_has_role, context_in_tenant};
pub use procedure_types::{
    ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr, ProcedurePolicyLiteral, ProcedurePredicate,
};
pub use read_types::{PolicyExpr, PolicyLiteral, ReadPolicy, ReadPredicate, RelationQuantifier};

#[cfg(test)]
mod tests {
    use super::*;
    use cratestack_core::{CoolContext, CoolError, Value};
    use std::collections::BTreeMap;

    struct NoArgs;
    impl ProcedureArgs for NoArgs {
        fn procedure_arg_value(&self, _field: &str) -> Option<Value> {
            None
        }
    }

    fn literal_policy(value: bool) -> ProcedurePolicy {
        ProcedurePolicy {
            expr: ProcedurePolicyExpr::Predicate(ProcedurePredicate::Literal(value)),
        }
    }

    /// `@allow(true)` (`ProcedurePredicate::Literal(true)`) must authorize
    /// every caller, including an unauthenticated one — the exact "public
    /// procedure" case a bare `true` clause is meant to express (see
    /// `ProcedurePredicate::Literal`'s docs). Before this variant existed,
    /// the only way to express "public" was two `@allow` clauses covering
    /// `auth() == null` / `auth() != null`; this proves the direct spelling
    /// is behaviourally equivalent for the case that matters.
    #[test]
    fn literal_true_allows_unauthenticated_callers() {
        let unauthenticated = CoolContext::anonymous();
        assert!(!unauthenticated.is_authenticated());
        let result = authorize_procedure(&[literal_policy(true)], &[], &NoArgs, &unauthenticated);
        assert!(result.is_ok(), "expected @allow(true) to allow: {result:?}");
    }

    /// `@allow(true)` must also allow an authenticated caller — it is
    /// unconditional, not merely "unauthenticated is fine too".
    #[test]
    fn literal_true_allows_authenticated_callers() {
        let authenticated = CoolContext::authenticated([]);
        let result = authorize_procedure(&[literal_policy(true)], &[], &NoArgs, &authenticated);
        assert!(result.is_ok(), "expected @allow(true) to allow: {result:?}");
    }

    /// `@deny(true)` (`ProcedurePredicate::Literal(true)` in a deny clause)
    /// must refuse unconditionally, mirroring `@allow(true)`'s unconditional
    /// accept.
    #[test]
    fn literal_true_in_deny_refuses_unconditionally() {
        let ctx = CoolContext::authenticated([]);
        let result = authorize_procedure(
            &[literal_policy(true)],
            &[literal_policy(true)],
            &NoArgs,
            &ctx,
        );
        assert!(matches!(result, Err(CoolError::Forbidden(_))));
    }

    /// `@allow(false)` never matches, so with no other `@allow` clause the
    /// procedure is unconditionally closed — same outcome as an empty
    /// `ALLOW_POLICIES` list, reached a different way.
    #[test]
    fn literal_false_never_allows() {
        let ctx = CoolContext::authenticated([]);
        let result = authorize_procedure(&[literal_policy(false)], &[], &NoArgs, &ctx);
        assert!(matches!(result, Err(CoolError::Forbidden(_))));
    }

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
