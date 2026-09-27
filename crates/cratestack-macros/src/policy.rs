mod ast;
mod attribute_audit;
mod auth;
mod model;
mod procedure;

pub(crate) use attribute_audit::{check_attribute, check_count, policy_names_in};
pub(crate) use auth::find_auth_field;
pub(crate) use model::{
    audit_model_policies, generate_denies_for_action, generate_denies_for_actions,
    generate_policies_for_action, generate_policies_for_actions,
};
pub(crate) use procedure::{
    PolicySubject, generate_procedure_policy, parse_procedure_allow_expression,
    parse_procedure_deny_expression,
};

/// Attribute names a procedure or query reader takes for a policy
/// (`attribute_audit`). `authorize` is included for queries too: a query
/// has no reader for it, so one there can only be a skipped rule.
pub(crate) const PROCEDURE_POLICY_NAMES: &[&str] = &["allow", "deny", "authorize"];
