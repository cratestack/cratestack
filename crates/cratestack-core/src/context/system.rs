//! SPIKE (`spike/b1-internal-actions`): the trusted/service principal
//! that model policies **name** via `auth().isSystem()`, rather than a
//! blanket "skip policy" bypass flag.
//!
//! The design constraint this file exists to satisfy: *obtaining a
//! system context must not be possible from a request-derived
//! context*. That is enforced structurally, not by convention:
//!
//! - [`SystemContext`] is the only public way to produce a
//!   [`CoolContext`] whose private `system` flag is set.
//! - It has no `From<CoolContext>`, no `TryFrom<CoolContext>`, and no
//!   constructor that accepts a caller-supplied `CoolContext`. There
//!   is therefore no function anywhere that turns an inbound request's
//!   context into a system one.
//! - It is not `Deserialize`, and `CoolContext::system` is
//!   `#[serde(skip)]`, so the marker cannot arrive over a wire.
//!
//! Fail-closed follows from the policy side: `is_system()` only ever
//! *satisfies a predicate a schema wrote down*. A model that never
//! names `auth().isSystem()` gains nothing from a system caller — the
//! default-deny in `push_allow_policy_query` / `evaluate_create_policies`
//! still applies.

use std::collections::BTreeMap;

use crate::value::Value;

use super::{CoolAuthIdentity, CoolContext, PrincipalContext};

/// A context representing trusted in-process/server code rather than
/// an end user.
///
/// Deliberately a distinct type from [`CoolContext`] so that "this
/// call runs as the system" is visible in a function signature and
/// greppable, instead of being a boolean threaded through call sites.
/// Borrow the inner context with [`SystemContext::context`] to hand it
/// to the ORM.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemContext {
    inner: CoolContext,
}

impl SystemContext {
    /// A system context attributed to a named service. The name is
    /// recorded as the `service` claim so audit trails can tell which
    /// piece of server code acted.
    pub fn for_service(service: impl Into<String>) -> Self {
        let service = service.into();
        let mut fields = BTreeMap::new();
        fields.insert("service".to_owned(), Value::String(service.clone()));
        fields.insert("id".to_owned(), Value::String(format!("system:{service}")));

        Self {
            inner: CoolContext {
                auth: Some(CoolAuthIdentity {
                    fields: fields.clone(),
                }),
                principal: Some(PrincipalContext::from_claims(fields)),
                extensions: BTreeMap::new(),
                system: true,
            },
        }
    }

    /// Borrow the underlying context to pass to the query layer.
    pub fn context(&self) -> &CoolContext {
        &self.inner
    }

    /// Consume into the underlying context.
    ///
    /// Note this is the *only* direction that exists. There is no
    /// inverse.
    pub fn into_context(self) -> CoolContext {
        self.inner
    }
}

impl AsRef<CoolContext> for SystemContext {
    fn as_ref(&self) -> &CoolContext {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_context_is_system_and_authenticated() {
        let ctx = SystemContext::for_service("ledger-worker");
        assert!(ctx.context().is_system());
        assert!(ctx.context().is_authenticated());
        assert_eq!(
            ctx.context().auth_field("service"),
            Some(&Value::String("ledger-worker".to_owned()))
        );
    }

    #[test]
    fn request_derived_contexts_are_never_system() {
        assert!(!CoolContext::anonymous().is_system());
        assert!(
            !CoolContext::authenticated([(
                "subjectId".to_owned(),
                Value::String("u-1".to_owned())
            )])
            .is_system()
        );
    }

    /// The wire is the interesting attack surface: if `system` were
    /// serialized, anything that round-trips a `CoolContext` (RPC
    /// envelopes, cached principals) would let a client assert it.
    #[test]
    fn system_flag_does_not_survive_serde_round_trip() {
        let system = SystemContext::for_service("ledger-worker").into_context();
        assert!(system.is_system());

        let json = serde_json::to_string(&system).expect("context should serialize");
        // Check for the *key*, not the substring — the service name
        // this fixture uses legitimately puts "system:" inside a claim
        // value.
        let encoded: serde_json::Value =
            serde_json::from_str(&json).expect("context should serialize to an object");
        assert!(
            encoded
                .as_object()
                .expect("context serializes as an object")
                .get("system")
                .is_none(),
            "system marker must not appear on the wire: {json}"
        );

        let decoded: CoolContext = serde_json::from_str(&json).expect("context should deserialize");
        assert!(
            !decoded.is_system(),
            "a deserialized context must never be a system context"
        );
    }

    /// A hand-forged payload that explicitly sets `system` must not be
    /// honoured either.
    #[test]
    fn forged_system_field_in_payload_is_ignored() {
        let decoded: CoolContext =
            serde_json::from_str(r#"{"auth":null,"principal":null,"extensions":{},"system":true}"#)
                .expect("unknown/skipped field should be ignored");
        assert!(!decoded.is_system());
    }
}
