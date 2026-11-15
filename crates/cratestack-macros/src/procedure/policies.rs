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
use crate::policy::{parse_procedure_allow_expression, parse_procedure_deny_expression};

/// `@allow` expressions, `@deny` expressions, and one
/// `authorize_with_db` check per `@authorize`.
pub(super) struct ProcedurePolicies<'a> {
    pub(super) allow: Vec<&'a str>,
    pub(super) deny: Vec<&'a str>,
    pub(super) authorizers: Vec<proc_macro2::TokenStream>,
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
    // The audit is `cratestack_policy::audit_procedure_attributes`, the one
    // `cratestack contract ops` also runs, so the two cannot drift on which
    // attributes read as a dropped policy. It runs first; the loop below only
    // lowers.
    cratestack_policy::audit_procedure_attributes(&owner, &procedure.attributes)?;
    for attribute in &procedure.attributes {
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
    }
    Ok(policies)
}
