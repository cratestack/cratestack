//! Defence in depth for `@@allow` / `@@deny` (GHSA-69g4-xvcm-vm2j): the
//! model-side counterpart of `crate::policy::attribute_audit`.
//!
//! [`super::parse_policy_expression`] returns `None` — and the rule is
//! skipped — both for a spelling it does not read (`@@deny ("read", …)`,
//! `@@Deny(…)`, a trailing `;`) and for an action it is not generating
//! (`@@deny("create", …)` while building the read slot). The second is
//! how one attribute reaches only its own slots, so `None` alone cannot
//! mean "dropped". Here every attribute that reads as `allow`/`deny` is
//! checked against *all* the actions the descriptor generates: if no slot
//! takes it, it would apply nowhere, and that is a compile error.

use cratestack_core::Model;

use super::parse_policy_expression;
use crate::policy::attribute_audit::{check_attribute, check_count, policy_names_in};

const MODEL_POLICY_NAMES: &[&str] = &["allow", "deny"];

/// Checks `model`'s policy attributes, where `actions` are the action
/// names the caller generates slots for (`all` is always accepted).
pub(crate) fn audit_model_policies(
    owner: &str,
    model: &Model,
    actions: &[&str],
) -> Result<(), String> {
    let mut named = 0;
    let mut produced = 0;
    for attribute in &model.attributes {
        named += policy_names_in(&attribute.raw, MODEL_POLICY_NAMES);
        let read = ["@@allow", "@@deny"]
            .into_iter()
            .filter_map(|directive| parse_policy_expression(&attribute.raw, directive, actions))
            .collect::<Result<Vec<_>, _>>()?
            .len();
        check_attribute(owner, attribute, MODEL_POLICY_NAMES, read)?;
        produced += read;
    }
    check_count(owner, named, produced)
}
