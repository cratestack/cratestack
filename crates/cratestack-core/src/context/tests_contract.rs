//! The principal contract (ADR 0021 §2): the builder fills what
//! `from_principal` fills, the typed readers answer the reserved names and
//! read a mistyped claim as absent, and `subject()` is the one "who".

use std::collections::BTreeMap;

use super::*;

fn full() -> CratestackContext {
    CratestackContext::builder("user-42")
        .roles(["admin", "teller"])
        .tenant("acme")
        .auth_time(1_793_000_000)
        .amr(["hwk", "user"])
        .acr("urn:example:loa:2")
        .cnf_jkt("0ZcOCORZNYy-DWpqq30jZyJGHTN0d2HglBV3uiguA4I")
        .cnf("ckt", Value::String("cose-thumb".to_owned()))
        .key_role(KeyRole::UserPresent)
        .expires_at(1_793_000_600)
        .claim("kycStatus", Value::String("verified".to_owned()))
        .build()
}

#[test]
fn the_builder_fills_what_from_principal_fills() {
    let expected = CratestackContext::from_principal(Some(serde_json::json!({
        "sub": "user-42",
        "roles": ["admin", "teller"],
        "tenant": { "id": "acme" },
        "auth_time": 1_793_000_000,
        "amr": ["hwk", "user"],
        "acr": "urn:example:loa:2",
        "cnf": { "jkt": "0ZcOCORZNYy-DWpqq30jZyJGHTN0d2HglBV3uiguA4I", "ckt": "cose-thumb" },
        "key_role": "user_present",
        "exp": 1_793_000_600,
        "kycStatus": "verified",
    })))
    .unwrap();
    let built = full();
    assert_eq!(built, expected);
    assert!(built.is_authenticated());
    assert!(!built.is_system());
    assert!(built.verified_signer().is_none());
    // The tenant is a facet, so the existing readers see it.
    assert_eq!(built.tenant_id(), Some("acme"));
    assert_eq!(
        built.auth_field("tenant.id"),
        Some(&Value::String("acme".to_owned()))
    );
    assert_eq!(
        built.auth_field("kycStatus"),
        Some(&Value::String("verified".to_owned()))
    );
}

#[test]
fn the_typed_readers_answer_the_reserved_names() {
    let ctx = full();
    assert_eq!(ctx.subject().as_deref(), Some("user-42"));
    assert_eq!(ctx.principal_actor_id().as_deref(), Some("user-42"));
    assert_eq!(ctx.roles(), Some(vec!["admin", "teller"]));
    assert_eq!(ctx.auth_time(), Some(1_793_000_000));
    assert_eq!(ctx.amr(), Some(vec!["hwk", "user"]));
    assert_eq!(ctx.acr(), Some("urn:example:loa:2"));
    assert_eq!(
        ctx.cnf_jkt(),
        Some("0ZcOCORZNYy-DWpqq30jZyJGHTN0d2HglBV3uiguA4I")
    );
    assert_eq!(
        ctx.cnf("ckt"),
        Some(&Value::String("cose-thumb".to_owned()))
    );
    assert_eq!(ctx.key_role(), Some(KeyRole::UserPresent));
    assert_eq!(ctx.expires_at(), Some(1_793_000_600));

    let anonymous = CratestackContext::anonymous();
    assert_eq!(anonymous.subject(), None);
    assert_eq!(anonymous.roles(), None);
    assert_eq!(anonymous.key_role(), None);
}

#[test]
fn a_mistyped_reserved_claim_reads_as_absent() {
    let ctx = CratestackContext::builder("u")
        .claim("roles", Value::String("admin".to_owned()))
        .claim(
            "amr",
            Value::List(vec![Value::String("hwk".to_owned()), Value::Int(1)]),
        )
        .claim("auth_time", Value::String("1793000000".to_owned()))
        .claim("acr", Value::Int(2))
        .claim("cnf", Value::String("jkt".to_owned()))
        .claim("key_role", Value::String("sometimes".to_owned()))
        .claim("exp", Value::Float(1.0))
        .build();
    assert_eq!(ctx.roles(), None);
    assert_eq!(ctx.amr(), None);
    assert_eq!(ctx.auth_time(), None);
    assert_eq!(ctx.acr(), None);
    assert_eq!(ctx.cnf("jkt"), None);
    assert_eq!(ctx.cnf_jkt(), None);
    assert_eq!(ctx.key_role(), None);
    assert_eq!(ctx.expires_at(), None);
}

#[test]
fn tenant_and_cnf_merge_into_an_existing_map() {
    let tenant = Value::Map(BTreeMap::from([(
        "slug".to_owned(),
        Value::String("acme-co".to_owned()),
    )]));
    let ctx = CratestackContext::builder("u")
        .claim("tenant", tenant)
        .tenant("acme")
        .cnf("kid", Value::String("k-1".to_owned()))
        .cnf_jkt("t")
        .build();
    assert_eq!(ctx.tenant_id(), Some("acme"));
    assert_eq!(
        ctx.auth_field("tenant.slug"),
        Some(&Value::String("acme-co".to_owned()))
    );
    assert_eq!(ctx.cnf("kid"), Some(&Value::String("k-1".to_owned())));
    assert_eq!(ctx.cnf_jkt(), Some("t"));
}

fn claims(entries: serde_json::Value) -> CratestackContext {
    CratestackContext::from_principal(Some(entries)).unwrap()
}

#[test]
fn subject_reads_sub_then_actor_id_then_id() {
    let all = claims(serde_json::json!({ "sub": "s", "actor": { "id": "a" }, "id": "i" }));
    assert_eq!(all.subject().as_deref(), Some("s"));
    let no_sub = claims(serde_json::json!({ "actor": { "id": "a" }, "id": "i" }));
    assert_eq!(no_sub.subject().as_deref(), Some("a"));
    let id_only = claims(serde_json::json!({ "id": "i" }));
    assert_eq!(id_only.subject().as_deref(), Some("i"));

    // An integer `actor.id` or `id` renders as its decimal digits (MCP's
    // rule); `sub` is a string claim, so an integer `sub` reads as absent.
    let int_id = claims(serde_json::json!({ "sub": 9, "id": 7 }));
    assert_eq!(int_id.subject().as_deref(), Some("7"));
    let int_actor = claims(serde_json::json!({ "actor": { "id": 12 } }));
    assert_eq!(int_actor.subject().as_deref(), Some("12"));

    // Any other kind reads as absent, and the next candidate is tried.
    let bool_actor = claims(serde_json::json!({ "actor": { "id": true }, "id": "i" }));
    assert_eq!(bool_actor.subject().as_deref(), Some("i"));
    assert_eq!(claims(serde_json::json!({ "id": 7.5 })).subject(), None);

    let system = SystemContext::for_service("svc").into_context();
    assert_eq!(system.subject().as_deref(), Some("system:svc"));
}

#[test]
fn key_role_spells_the_claim_values() {
    for role in [KeyRole::Ambient, KeyRole::UserPresent] {
        assert_eq!(KeyRole::from_claim(role.as_str()), Some(role));
        assert_eq!(
            serde_json::to_value(role).unwrap(),
            serde_json::Value::String(role.as_str().to_owned())
        );
    }
    assert_eq!(KeyRole::Ambient.as_str(), "ambient");
    assert_eq!(KeyRole::UserPresent.as_str(), "user_present");
    assert_eq!(KeyRole::from_claim("UserPresent"), None);
}

#[test]
fn id_is_not_reserved() {
    assert!(RESERVED_CLAIMS.iter().all(|claim| claim.path != "id"));
    assert!(RESERVED_CLAIMS.iter().any(|claim| claim.path == "sub"));
}
