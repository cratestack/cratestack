//! Unit tests for `idempotency.rs`: the `_meta` key rules and the identity
//! a keyed or rate-limited call is scoped to. A sibling file, as
//! `cratestack-exec` lays out its own, to keep `idempotency.rs` under the
//! 200-line ceiling.

use cratestack_core::{
    CratestackContext, PrincipalContext, PrincipalFacet, SystemContext, Value as ClaimValue,
};
use rmcp::model::RequestMetaObject;
use serde_json::{Value, json};

use crate::idempotency::{idempotency_key, namespace, principal_id};

fn meta(value: Value) -> RequestMetaObject {
    let mut meta = RequestMetaObject::new();
    meta.insert(crate::IDEMPOTENCY_KEY_META.to_owned(), value);
    meta
}

#[test]
fn a_key_follows_the_idempotency_key_header_rules() {
    let empty = RequestMetaObject::new();
    assert_eq!(idempotency_key(&empty, None).unwrap(), None);
    assert_eq!(
        idempotency_key(&meta(json!("  k-1 ")), None).unwrap(),
        Some("k-1".to_owned())
    );
    assert_eq!(
        idempotency_key(&empty, Some(&meta(json!("k-2")))).unwrap(),
        Some("k-2".to_owned())
    );
    for bad in [json!(7), json!("   "), json!("x".repeat(256)), json!("é")] {
        let error = idempotency_key(&meta(bad.clone()), None).unwrap_err();
        assert_eq!(error.code(), "BAD_REQUEST", "{bad} must be refused");
    }
}

#[test]
fn no_actor_id_means_no_namespace() {
    assert_eq!(namespace(Some("u-1")).unwrap(), "mcp:u-1");
    assert_eq!(namespace(None).unwrap_err().code(), "PRECONDITION_FAILED");
    assert_eq!(
        namespace(Some("")).unwrap_err().code(),
        "PRECONDITION_FAILED"
    );
}

fn id(value: ClaimValue) -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), value)])
}

/// `principal_id` restates `principal_actor_id`'s lookup so it can accept
/// an `Int`. For every string-id shape the two must answer the same, or the
/// restatement has drifted from core.
#[test]
fn a_string_id_agrees_with_principal_actor_id() {
    let mut facet = PrincipalContext::from_claims(
        [("id".to_owned(), ClaimValue::String("claim".to_owned()))].into(),
    );
    facet.actor = Some(PrincipalFacet {
        fields: [("id".to_owned(), ClaimValue::String("facet".to_owned()))].into(),
    });
    let contexts = [
        id(ClaimValue::String("u-1".to_owned())),
        SystemContext::for_service("svc").into_context(),
        CratestackContext::with_principal(facet),
        CratestackContext::anonymous(),
        id(ClaimValue::Bool(true)),
    ];
    for ctx in &contexts {
        assert_eq!(
            principal_id(ctx).as_deref(),
            ctx.principal_actor_id(),
            "{ctx:?}"
        );
    }
    assert_eq!(principal_id(&contexts[2]).as_deref(), Some("facet"));
}

#[test]
fn an_int_id_is_an_identity_and_other_kinds_are_not() {
    assert_eq!(principal_id(&id(ClaimValue::Int(7))).as_deref(), Some("7"));
    assert_eq!(principal_id(&id(ClaimValue::Float(7.0))), None);
    assert_eq!(principal_id(&id(ClaimValue::Null)), None);
}
