//! The trusted/service principal that model policies **name** via
//! `auth().isSystem()`, rather than a blanket "skip policy" bypass
//! flag (issue #486 / webank-context ADR 0038 blocker B1).
//!
//! The design constraint this file exists to satisfy: *obtaining a
//! system context must not be possible from a request-derived
//! context*. That is enforced structurally, not by convention:
//!
//! - [`SystemContext`] is the only public way to produce a
//!   [`CratestackContext`] whose private `system` flag is set.
//! - It has no `From<CratestackContext>`, no `TryFrom<CratestackContext>`, and no
//!   constructor that accepts a caller-supplied `CratestackContext`. There is
//!   therefore no function anywhere — in this crate or any downstream
//!   one — that turns an inbound request's context into a system one.
//!   An `AuthProvider::authenticate` implementation, which is the only
//!   place a `CratestackContext` is ever built from a request, has no way to
//!   reach this type at all.
//! - It is not `Deserialize`, and `CratestackContext::system` is
//!   `#[serde(skip)]`, so the marker cannot arrive over a wire (RPC
//!   envelope, cached principal, client-state-store round trip, ...).
//!
//! Fail-closed follows from the *policy* side, not from this type:
//! `is_system()` only ever *satisfies a predicate a schema wrote down*
//! (`ReadPredicate::AuthIsSystem`, matched in
//! `cratestack_sqlx::query::support::create::evaluate_input_predicate`,
//! `query::support::policy_predicate::push_policy_predicate`, and
//! `render::policy_predicate::render_policy_predicate`). A model that
//! never names `auth().isSystem()` in an `@@allow` clause never emits
//! that predicate at all — see
//! `cratestack_macros::policy::model::tests_system_principal` — so a
//! system caller gains nothing on it: the model's existing default-deny
//! / owner-scoped rules apply exactly as they would to any other caller
//! lacking the claims those rules check for.

use std::collections::BTreeMap;

use crate::value::Value;

use super::{CratestackAuthIdentity, CratestackContext, PrincipalContext};

/// A context representing trusted in-process/server code (a procedure,
/// a worker, a reconciliation job) rather than an end user.
///
/// Deliberately a distinct type from [`CratestackContext`] so that "this call
/// runs as the system" is visible in a function signature and
/// greppable, instead of being a boolean threaded through call sites
/// the way `db.model().unchecked().update(...)` would have been. Borrow
/// the inner context with [`SystemContext::context`] to hand it to the
/// ORM (`cool.model().update(id).run(system.context())`), or consume it
/// with [`SystemContext::into_context`] where an owned `CratestackContext` is
/// required.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemContext {
    inner: CratestackContext,
}

impl SystemContext {
    /// A system context attributed to a named service. The name is
    /// recorded as both the `service` claim and (prefixed) the `id`
    /// claim, so it flows through unchanged into
    /// `cratestack_sqlx::audit::actor_from_context` — which reads
    /// `principal.claims.id` for `AuditActor::id` and the full claims
    /// map for `AuditActor::claims` — without any audit-path code
    /// needing to know a system caller is a distinct kind of caller.
    /// An audit row produced by a system write reads
    /// `actor.id = "system:<service>"`, which is how design constraint
    /// #3 (auditability) is met: no new audit machinery, just a
    /// deliberately-shaped actor identity flowing through the existing
    /// one.
    pub fn for_service(service: impl Into<String>) -> Self {
        let service = service.into();
        let mut fields = BTreeMap::new();
        fields.insert("service".to_owned(), Value::String(service.clone()));
        fields.insert("id".to_owned(), Value::String(format!("system:{service}")));

        Self {
            inner: CratestackContext {
                auth: Some(CratestackAuthIdentity {
                    fields: fields.clone(),
                }),
                principal: Some(PrincipalContext::from_claims(fields)),
                extensions: BTreeMap::new(),
                system: true,
            },
        }
    }

    /// Borrow the underlying context to pass to the query layer.
    pub fn context(&self) -> &CratestackContext {
        &self.inner
    }

    /// Consume into the underlying context.
    ///
    /// Note this is the *only* direction that exists: `CratestackContext ->
    /// SystemContext` has no constructor anywhere. That asymmetry is
    /// the whole security property this module provides.
    pub fn into_context(self) -> CratestackContext {
        self.inner
    }
}

impl AsRef<CratestackContext> for SystemContext {
    fn as_ref(&self) -> &CratestackContext {
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
        assert!(!CratestackContext::anonymous().is_system());
        assert!(
            !CratestackContext::authenticated([(
                "subjectId".to_owned(),
                Value::String("u-1".to_owned())
            )])
            .is_system()
        );
    }

    /// The wire is the interesting attack surface: if `system` were
    /// serialized, anything that round-trips a `CratestackContext` (RPC
    /// envelopes, cached principals) would let a client assert it.
    #[test]
    fn system_flag_does_not_survive_serde_round_trip() {
        let system = SystemContext::for_service("ledger-worker").into_context();
        assert!(system.is_system());

        let json = serde_json::to_string(&system).expect("context should serialize");
        // Check for the *key*, not the substring — the service name
        // this fixture uses legitimately puts "system:" inside a claim
        // value, so a substring check would pass for the wrong reason.
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

        let decoded: CratestackContext =
            serde_json::from_str(&json).expect("context should deserialize");
        assert!(
            !decoded.is_system(),
            "a deserialized context must never be a system context"
        );
    }

    /// A hand-forged payload that explicitly sets `system` must not be
    /// honoured either — this is the forgery test design constraint #4
    /// asks for: nothing an HTTP caller controls can produce a system
    /// context, and a wire payload is the most direct thing an HTTP
    /// caller controls.
    #[test]
    fn forged_system_field_in_payload_is_ignored() {
        let decoded: CratestackContext =
            serde_json::from_str(r#"{"auth":null,"principal":null,"extensions":{},"system":true}"#)
                .expect("unknown/skipped field should be ignored");
        assert!(!decoded.is_system());
    }
}
