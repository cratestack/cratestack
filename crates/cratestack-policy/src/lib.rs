//! Read- and procedure-policy types plus evaluator.

mod analysis;
mod audit;
mod compare;
mod eval;
mod predicate;
mod procedure_types;
mod read_types;
mod slots;
mod syntax;
#[cfg(test)]
mod tests_authorize_procedure;
#[cfg(test)]
mod tests_claim_comparison;
#[cfg(test)]
mod tests_claim_comparison_deny;
#[cfg(test)]
mod tests_claim_comparison_edges;
#[cfg(test)]
mod tests_claim_comparison_unchanged;
#[cfg(test)]
mod tests_compare;
#[cfg(test)]
mod tests_has_role_contract;
#[cfg(test)]
mod tests_procedure_context_predicates;
#[cfg(test)]
mod tests_procedure_input_predicates;
#[cfg(test)]
mod tests_read_types;
mod truth;

pub use analysis::{OpPolicyAnalysis, analyse_op_policy};
pub use audit::{
    MODEL_POLICY_NAMES, PROCEDURE_POLICY_NAMES, audit_model_attributes, audit_procedure_attributes,
    check_attribute, check_count, policy_names_in,
};
pub use eval::{authorize_procedure, authorize_query, context_has_role, context_in_tenant};
pub use procedure_types::{
    ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr, ProcedurePolicyLiteral, ProcedurePredicate,
};
pub use read_types::{PolicyExpr, PolicyLiteral, ReadPolicy, ReadPredicate, RelationQuantifier};
pub use slots::{ModelPolicySlot, model_verb_policy_slots};
pub use syntax::{
    AUTH_IS_NULL_TERM, AUTH_NOT_NULL_TERM, PolicyAst, authorize_attribute_inner,
    is_auth_is_system_term, model_rule_expression, parse_builtin_policy_call, parse_policy_ast,
    parse_string_literal, procedure_rule_expression,
};
