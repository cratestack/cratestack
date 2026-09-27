//! Reading a procedure's policy attributes — `@allow`, `@deny`,
//! `@authorize` — into the pieces `generate_procedure_module` emits.
//!
//! GHSA-69g4-xvcm-vm2j: the exact readers return `None` for any other
//! spelling, which used to drop the rule silently. Every attribute is also
//! read loosely (`crate::policy::attribute_audit`), and one that reads as a
//! policy but produced none, or a count that does not match, is an error
//! that becomes a `compile_error!`.

use cratestack_core::{Model, Procedure, TypeDecl};

use super::authorizer::{generate_procedure_model_authorizer, parse_procedure_model_authorizer};
use crate::policy::{
    PROCEDURE_POLICY_NAMES, check_attribute, check_count, parse_procedure_allow_expression,
    parse_procedure_deny_expression, policy_names_in,
};

/// `@allow` expressions, `@deny` expressions, and one
/// `authorize_with_db` check per `@authorize`.
pub(super) struct ProcedurePolicies<'a> {
    pub(super) allow: Vec<&'a str>,
    pub(super) deny: Vec<&'a str>,
    pub(super) authorizers: Vec<proc_macro2::TokenStream>,
}

impl ProcedurePolicies<'_> {
    fn len(&self) -> usize {
        self.allow.len() + self.deny.len() + self.authorizers.len()
    }
}

pub(super) fn collect_procedure_policies<'a>(
    procedure: &'a Procedure,
    models: &[Model],
    types: &[TypeDecl],
) -> Result<ProcedurePolicies<'a>, String> {
    let owner = format!("procedure `{}`", procedure.name);
    let mut policies = ProcedurePolicies {
        allow: Vec::new(),
        deny: Vec::new(),
        authorizers: Vec::new(),
    };
    let mut named = 0;
    for attribute in &procedure.attributes {
        named += policy_names_in(&attribute.raw, PROCEDURE_POLICY_NAMES);
        let before = policies.len();
        if let Some(expression) = parse_procedure_allow_expression(&attribute.raw) {
            policies.allow.push(expression?);
        }
        if let Some(expression) = parse_procedure_deny_expression(&attribute.raw) {
            policies.deny.push(expression?);
        }
        if let Some(authorizer) = parse_procedure_model_authorizer(&attribute.raw) {
            policies
                .authorizers
                .push(generate_procedure_model_authorizer(
                    authorizer?,
                    procedure,
                    models,
                    types,
                )?);
        }
        check_attribute(
            &owner,
            attribute,
            PROCEDURE_POLICY_NAMES,
            policies.len() - before,
        )?;
    }
    check_count(&owner, named, policies.len())?;
    Ok(policies)
}
