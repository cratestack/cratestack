use std::collections::BTreeMap;

use super::*;

#[test]
fn auth_field_prefers_exact_key_before_dotted_lookup() {
    let ctx = CoolContext::authenticated([
        ("tenant.slug".to_owned(), Value::String("exact".to_owned())),
        (
            "tenant".to_owned(),
            Value::Map(BTreeMap::from([(
                "slug".to_owned(),
                Value::String("nested".to_owned()),
            )])),
        ),
    ]);

    assert_eq!(
        ctx.auth_field("tenant.slug"),
        Some(&Value::String("exact".to_owned()))
    );
}

#[test]
fn auth_field_resolves_nested_map_paths() {
    let ctx = CoolContext::from_principal(Some(serde_json::json!({
        "tenant": {
            "slug": "acme",
            "owner": { "id": 7 }
        }
    })))
    .expect("principal should bind");

    assert_eq!(
        ctx.auth_field("tenant.slug"),
        Some(&Value::String("acme".to_owned()))
    );
    assert_eq!(ctx.auth_field("tenant.owner.id"), Some(&Value::Int(7)));
    assert!(ctx.auth_field("tenant.owner.missing").is_none());
}

#[test]
fn from_principal_promotes_actor_session_and_tenant_facets() {
    let ctx = CoolContext::from_principal(Some(serde_json::json!({
        "actor": { "id": "usr_1" },
        "session": { "id": "sess_1" },
        "tenant": { "id": "org_1" },
        "role": "admin"
    })))
    .expect("principal should bind");

    let principal = ctx.principal.expect("principal should exist");
    assert_eq!(
        principal.actor.as_ref().and_then(|facet| facet.fields.get("id")),
        Some(&Value::String("usr_1".to_owned()))
    );
    assert_eq!(
        principal.session.as_ref().and_then(|facet| facet.fields.get("id")),
        Some(&Value::String("sess_1".to_owned()))
    );
    assert_eq!(
        principal.tenant.as_ref().and_then(|facet| facet.fields.get("id")),
        Some(&Value::String("org_1".to_owned()))
    );
    assert_eq!(
        principal.claims.get("role"),
        Some(&Value::String("admin".to_owned()))
    );
}

#[test]
fn request_id_round_trip_through_extensions() {
    let ctx = CoolContext::anonymous().with_request_id("trace-123");
    assert_eq!(ctx.request_id(), Some("trace-123"));
}

#[test]
fn client_ip_round_trip_through_extensions() {
    let ctx = CoolContext::anonymous().with_client_ip("192.0.2.43");
    assert_eq!(ctx.client_ip(), Some("192.0.2.43"));
}
