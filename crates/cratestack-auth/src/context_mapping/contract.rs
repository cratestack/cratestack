//! The contract-only mapping (ADR 0021 §2, item 2): the principal contract's
//! names, plus only the free-form claims a deployment selects, for an
//! application that does not read the product shape.

use cratestack_core::{CratestackContext, CratestackError, Value};

use crate::id_token::RequestPrincipal;

use super::principal_to_cratestack_context;

/// The contract's names for `principal`: `sub` (the user id, or
/// `svc:<key id>` for a service caller), `roles` (the verified `role`, or
/// `role` — default `caller` — for a service caller) and, for a user, `cnf`
/// (`kid`, plus `jkt` when the id token carried the key). Each name in
/// `selected` is then copied from the product mapping's top-level claims
/// when it has one: `id`, `role`, `clientId`, `enrollmentStatus`,
/// `boundKeyId`, `kycStatus`, `kycDossierId`, the `actor` and `session`
/// facets for a user, and `callerService`, `serviceName`, `actorType` for a
/// service caller ([`super::principal_to_cratestack_context`]). A name the
/// product mapping does not have is skipped.
///
/// With no user and `allow_transport_caller` false the context is anonymous,
/// as in the product mapping.
pub fn principal_to_contract_context<S: AsRef<str>>(
    principal: &RequestPrincipal,
    role: Option<&str>,
    allow_transport_caller: bool,
    selected: &[S],
) -> Result<CratestackContext, CratestackError> {
    let product = principal_to_cratestack_context(principal, role, allow_transport_caller)?;
    let mut builder = match principal.user.as_ref() {
        Some(user) => {
            let builder = CratestackContext::builder(user.user_id.clone())
                .roles([user.role.clone()])
                .cnf("kid", Value::String(user.bound_key_id.clone()));
            match &user.bound_key_jkt {
                Some(jkt) => builder.cnf_jkt(jkt.clone()),
                None => builder,
            }
        }
        None if allow_transport_caller => {
            CratestackContext::builder(format!("svc:{}", principal.transport.key_id))
                .roles([role.unwrap_or("caller")])
        }
        None => return Ok(CratestackContext::anonymous()),
    };
    // `product` is ours, so a selected claim is moved out of it, not cloned.
    let mut fields = product.auth.map(|auth| auth.fields).unwrap_or_default();
    for name in selected {
        if let Some(value) = fields.remove(name.as_ref()) {
            builder = builder.claim(name.as_ref(), value);
        }
    }
    Ok(builder.build())
}
