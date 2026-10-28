//! Who a session belongs to: the key re-authentication must preserve
//! (design §6.3, "Expiry and refresh") and the key sessions are counted
//! under (§6.6, "Sockets per principal").

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};

use cratestack_core::{CratestackContext, Value};

/// The default [`crate::PrincipalKeyFn`]: the principal's `actor.id`, else
/// the auth identity's `id`, else its `sub`. A context with none of them
/// is anonymous.
pub fn default_principal_key(ctx: &CratestackContext) -> Option<String> {
    let from_principal = ctx.principal.as_ref().and_then(|p| p.field("actor.id"));
    let from_auth = || {
        let fields = &ctx.auth.as_ref()?.fields;
        fields.get("id").or_else(|| fields.get("sub"))
    };
    from_principal.or_else(from_auth).map(value_key)
}

fn value_key(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Int(number) => number.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// What a session is counted under: the principal, or the remote IP for an
/// anonymous caller, so one address cannot hold unlimited anonymous
/// sessions either.
pub(crate) fn session_key(principal: Option<&str>, remote: IpAddr) -> String {
    match principal {
        Some(principal) => format!("principal:{principal}"),
        None => format!("anonymous:{remote}"),
    }
}

/// Open sessions per key, shared by every session of one server. A
/// poisoned lock is recovered rather than propagated: the guarded state is
/// a counter no panic can leave half-updated, and propagating would turn one
/// panicked task into a refusal of every later session.
#[derive(Debug, Default)]
pub(crate) struct SessionRegistry {
    open: Mutex<HashMap<String, usize>>,
}

/// One admitted session. Dropping it frees the slot, however the session
/// ended.
#[derive(Debug)]
pub(crate) struct SessionSlot {
    registry: Arc<SessionRegistry>,
    key: String,
}

impl SessionRegistry {
    pub(crate) fn admit(self: &Arc<Self>, key: String, limit: usize) -> Option<SessionSlot> {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        let count = open.entry(key.clone()).or_default();
        if *count >= limit {
            return None;
        }
        *count += 1;
        Some(SessionSlot {
            registry: Arc::clone(self),
            key,
        })
    }
}

impl Drop for SessionSlot {
    fn drop(&mut self) {
        let mut open = self
            .registry
            .open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = open.get_mut(&self.key) {
            *count -= 1;
            if *count == 0 {
                open.remove(&self.key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_prefers_actor_id_then_id_then_sub() {
        let by_id = CratestackContext::authenticated([("id".to_owned(), Value::Int(7))]);
        assert_eq!(default_principal_key(&by_id).as_deref(), Some("7"));
        let by_sub =
            CratestackContext::authenticated([("sub".to_owned(), Value::String("u-1".into()))]);
        assert_eq!(default_principal_key(&by_sub).as_deref(), Some("u-1"));
        assert_eq!(default_principal_key(&CratestackContext::anonymous()), None);
    }

    #[test]
    fn slots_are_bounded_per_key_and_freed_on_drop() {
        let registry = Arc::new(SessionRegistry::default());
        let a1 = registry.admit("a".into(), 2).unwrap();
        let _a2 = registry.admit("a".into(), 2).unwrap();
        assert!(registry.admit("a".into(), 2).is_none());
        assert!(
            registry.admit("b".into(), 2).is_some(),
            "other keys are unaffected"
        );
        drop(a1);
        assert!(registry.admit("a".into(), 2).is_some());
    }
}
