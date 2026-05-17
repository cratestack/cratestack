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
    use cratestack_core::{CoolContext, Value};
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
