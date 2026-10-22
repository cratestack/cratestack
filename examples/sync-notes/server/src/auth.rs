//! **Demo-only authentication.** The caller's identity is whatever the
//! `x-demo-user` header says, so the page can switch between Alice and Bob
//! without a login flow. Never ship this: swap in a real `AuthProvider`
//! (JWT, `cratestack-auth`, COSE device keys). Everything downstream — the
//! generated routes' policies and every sync protection — is keyed on the
//! context this returns, so it is the one piece that must be real.

use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_sync::server::SyncPrincipal;

#[derive(Clone)]
pub struct DemoAuth;

impl AuthProvider for DemoAuth {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, Self::Error> {
        let user = request
            .headers
            .get("x-demo-user")
            .and_then(|value| value.to_str().ok())
            .filter(|user| !user.is_empty() && user.len() <= 32);
        Ok(match user {
            Some(user) => {
                CratestackContext::authenticated([("id".to_owned(), Value::String(user.into()))])
            }
            None => CratestackContext::anonymous(),
        })
    }
}

/// One scope per user: a device syncs exactly its owner's notes. A shared
/// notebook would add `notebook:<id>` scopes here from a membership lookup.
pub fn principal(ctx: &CratestackContext) -> Option<SyncPrincipal> {
    match ctx.auth_field("id") {
        Some(Value::String(id)) => Some(SyncPrincipal {
            id: id.clone(),
            scopes: vec![format!("user:{id}")],
        }),
        _ => None,
    }
}
