//! Both mappings of a verified principal (ADR 0021 §2): the product one
//! keeps every existing claim and adds `sub`, `roles` and `cnf`; the
//! contract one emits those names and only the claims a deployment selects.

use std::collections::HashMap;

use chrono::{TimeZone, Utc};
use cratestack_core::{CratestackContext, Value};

use super::{PrincipalMapper, principal_to_contract_context, principal_to_cratestack_context};
use crate::id_token::{RequestPrincipal, UserPrincipal};
use crate::signed_request::SignedRequestPrincipal;

fn transport() -> SignedRequestPrincipal {
    SignedRequestPrincipal {
        key_id: "key-1".to_owned(),
        timestamp: Utc.timestamp_opt(1_793_000_000, 0).unwrap(),
        nonce: "n-1".to_owned(),
        id_jwt: None,
        alg: None,
        content_sha256: String::new(),
        via_id_token_pop: false,
    }
}

fn user(jkt: Option<&str>) -> RequestPrincipal {
    RequestPrincipal {
        transport: transport(),
        user: Some(UserPrincipal {
            user_id: "usr_1".to_owned(),
            audience: "aud".to_owned(),
            client_id: "app".to_owned(),
            issued_at: 1_793_000_000,
            expires_at: 1_793_000_600,
            bound_key_id: "key-1".to_owned(),
            bound_key_jkt: jkt.map(str::to_owned),
            profile_version: 3,
            enrollment_status: "active".to_owned(),
            kyc_status: Some("verified".to_owned()),
            role: "admin".to_owned(),
            main_email: None,
            main_phone: None,
            main_address: None,
            disclosed_claims: HashMap::new(),
        }),
    }
}

fn service() -> RequestPrincipal {
    RequestPrincipal {
        transport: transport(),
        user: None,
    }
}

fn string(ctx: &CratestackContext, path: &str) -> Option<String> {
    match ctx.auth_field(path)? {
        Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

#[test]
fn the_product_shape_also_carries_the_contract_names() {
    let ctx = principal_to_cratestack_context(&user(Some("jkt-1")), Some("caller"), false).unwrap();
    // Nothing that was there before is gone.
    for (path, expected) in [
        ("id", "usr_1"),
        ("actor.id", "usr_1"),
        ("role", "admin"),
        ("kycStatus", "verified"),
        ("session.boundKeyId", "key-1"),
        ("clientId", "app"),
    ] {
        assert_eq!(string(&ctx, path).as_deref(), Some(expected), "{path}");
    }
    assert_eq!(ctx.subject().as_deref(), Some("usr_1"));
    assert_eq!(ctx.roles(), Some(vec!["admin"]));
    assert_eq!(ctx.cnf("kid"), Some(&Value::String("key-1".to_owned())));
    assert_eq!(ctx.cnf_jkt(), Some("jkt-1"));

    let no_jwk = principal_to_cratestack_context(&user(None), None, false).unwrap();
    assert_eq!(no_jwk.cnf_jkt(), None);
    assert_eq!(no_jwk.cnf("kid"), Some(&Value::String("key-1".to_owned())));
}

#[test]
fn a_service_caller_is_svc_key_with_its_default_role() {
    let ctx = principal_to_cratestack_context(&service(), Some("caller"), true).unwrap();
    assert_eq!(ctx.subject().as_deref(), Some("svc:key-1"));
    assert_eq!(string(&ctx, "id").as_deref(), Some("svc:key-1"));
    assert_eq!(ctx.roles(), Some(vec!["caller"]));
    assert_eq!(string(&ctx, "role").as_deref(), Some("caller"));
    assert_eq!(ctx.cnf("kid"), None);

    let refused = principal_to_cratestack_context(&service(), Some("caller"), false).unwrap();
    assert!(!refused.is_authenticated());
}

#[test]
fn the_contract_mapping_emits_only_the_contract_and_the_selection() {
    let ctx = principal_to_contract_context(
        &user(Some("jkt-1")),
        Some("caller"),
        false,
        &["kycStatus", "session", "notAClaim"],
    )
    .unwrap();
    assert_eq!(ctx.subject().as_deref(), Some("usr_1"));
    assert_eq!(ctx.roles(), Some(vec!["admin"]));
    assert_eq!(ctx.cnf_jkt(), Some("jkt-1"));
    assert_eq!(string(&ctx, "kycStatus").as_deref(), Some("verified"));
    assert_eq!(string(&ctx, "session.boundKeyId").as_deref(), Some("key-1"));
    for absent in ["id", "role", "actor.id", "clientId", "notAClaim"] {
        assert_eq!(ctx.auth_field(absent), None, "{absent}");
    }

    let service =
        principal_to_contract_context::<&str>(&service(), Some("caller"), true, &[]).unwrap();
    assert_eq!(service.subject().as_deref(), Some("svc:key-1"));
    assert_eq!(service.roles(), Some(vec!["caller"]));
    assert_eq!(service.auth_field("role"), None);

    let refused =
        principal_to_contract_context::<&str>(&self::service(), None, false, &[]).unwrap();
    assert!(!refused.is_authenticated());
}

#[test]
fn the_default_mapper_is_the_product_shape() {
    let principal = user(None);
    let product = PrincipalMapper::default().map(&principal, Some("caller"), false);
    assert_eq!(
        product.unwrap(),
        principal_to_cratestack_context(&principal, Some("caller"), false).unwrap()
    );
    let contract = PrincipalMapper::contract(["kycStatus"])
        .map(&principal, Some("caller"), false)
        .unwrap();
    assert_eq!(contract.auth_field("id"), None);
    assert_eq!(string(&contract, "kycStatus").as_deref(), Some("verified"));
    let custom = PrincipalMapper::custom(|_, _, _| Ok(CratestackContext::builder("x").build()));
    assert_eq!(
        custom
            .map(&principal, None, false)
            .unwrap()
            .subject()
            .as_deref(),
        Some("x")
    );
    assert_eq!(
        format!("{:?}", PrincipalMapper::default()),
        "PrincipalMapper(\"product\")"
    );
}
