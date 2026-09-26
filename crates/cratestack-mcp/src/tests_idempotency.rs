//! Unit tests for `idempotency.rs`: the `_meta` key rules and the identity
//! a keyed or rate-limited call is scoped to. A sibling file, as
//! `cratestack-exec` lays out its own, to keep `idempotency.rs` under the
//! 200-line ceiling.

use cratestack_core::{
    CratestackContext, PrincipalContext, PrincipalFacet, SystemContext, Value as ClaimValue,
};
use rmcp::model::RequestMetaObject;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

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
    let string = |value: &str| id(ClaimValue::String(value.to_owned()));
    assert_eq!(namespace(&string("u-1")).unwrap(), hashed("mcp", "u-1"));
    assert_eq!(
        namespace(&CratestackContext::anonymous())
            .unwrap_err()
            .code(),
        "PRECONDITION_FAILED"
    );
    assert_eq!(
        namespace(&string("")).unwrap_err().code(),
        "PRECONDITION_FAILED"
    );
}

/// The cratestack#1039 maintainer decision, at the unit level: a user whose
/// `id` claim is literally a system id does not land in that service's
/// namespace. The end-to-end replay test is `tests/admission_prefix.rs`.
#[test]
fn a_user_claiming_a_system_id_is_not_in_the_system_namespace() {
    let system = SystemContext::for_service("svc").into_context();
    let impostor = id(ClaimValue::String("system:svc".to_owned()));
    assert_eq!(principal_id(&system), principal_id(&impostor));
    assert_eq!(
        namespace(&system).unwrap(),
        hashed("mcp-system", "system:svc")
    );
    assert_eq!(namespace(&impostor).unwrap(), hashed("mcp", "system:svc"));
}

/// The cratestack#1033 decision on #1071's first question: the id is
/// hashed, so it never reaches a store key verbatim, and a caller's MCP
/// budget is its own, never one of REST's buckets. REST's default keys
/// (`cratestack-axum`'s `ratelimit/key_fn.rs`) are `princ:<sha256>`,
/// `auth:<sha256>`, and peer-address keys; the MCP prefixes differ from
/// every one, whatever the id.
#[test]
fn an_mcp_namespace_is_never_a_rest_bucket() {
    let user = namespace(&id(ClaimValue::String("u-1".to_owned()))).unwrap();
    let system = namespace(&SystemContext::for_service("svc").into_context()).unwrap();
    for key in [&user, &system] {
        assert!(!key.contains("u-1") && !key.contains("svc"), "{key}");
        assert_eq!(key.rsplit(':').next().unwrap().len(), 64, "{key}");
        for rest in ["princ:", "auth:", "peer:", "ip:"] {
            assert!(!key.starts_with(rest), "{key} is a REST bucket");
        }
    }
    assert_ne!(user, format!("princ:{}", hex("u-1")));
}

/// `<prefix>:<sha256 hex of id>`, computed here rather than through the
/// crate's own helper, so a change to what is hashed fails these tests.
fn hashed(prefix: &str, id: &str) -> String {
    format!("{prefix}:{}", hex(id))
}

fn hex(id: &str) -> String {
    Sha256::digest(id.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
