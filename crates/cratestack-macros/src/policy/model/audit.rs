//! Defence in depth for `@@allow` / `@@deny` (GHSA-69g4-xvcm-vm2j): the
//! model-side counterpart of the procedure audit. The check itself is
//! `cratestack_policy::audit_model_attributes`, shared with
//! `cratestack contract ops` so the two refuse (or report) the same
//! attributes.
//!
//! The model reader returns `None` — and the rule is skipped — both for a
//! spelling it does not read (`@@deny ("read", …)`, `@@Deny(…)`, a trailing
//! `;`) and for an action it is not generating (`@@deny("create", …)` while
//! building the read slot). The second is how one attribute reaches only its
//! own slots, so `None` alone cannot mean "dropped". Every attribute that
//! reads as `allow`/`deny` is checked against *all* the actions the
//! descriptor generates: if no slot takes it, it would apply nowhere, and
//! that is a compile error.

use cratestack_core::Model;

/// Checks `model`'s policy attributes, where `actions` are the action
/// names the caller generates slots for (`all` is always accepted).
pub(crate) fn audit_model_policies(
    owner: &str,
    model: &Model,
    actions: &[&str],
) -> Result<(), String> {
    cratestack_policy::audit_model_attributes(owner, &model.attributes, actions)
}
