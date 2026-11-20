use super::*;

fn session(name: &str) -> RevocationKey {
    RevocationKey::Session(name.to_owned())
}

#[test]
fn default_keys_read_subject_and_a_string_sid() {
    use crate::CratestackContext;

    let ctx = CratestackContext::builder("u-1")
        .claim("sid", crate::Value::String("s-1".into()))
        .build();
    assert_eq!(
        default_revocation_keys(&ctx),
        vec![RevocationKey::Subject("u-1".into()), session("s-1")]
    );

    let numeric = CratestackContext::builder("u-1")
        .claim("sid", crate::Value::Int(7))
        .build();
    assert_eq!(
        default_revocation_keys(&numeric),
        vec![RevocationKey::Subject("u-1".into())]
    );
}

#[test]
fn a_signer_key_round_trips_through_its_wire_form() {
    let key = RevocationKey::Signer([7; 32]);
    let (kind, value) = key.parts();
    assert_eq!(RevocationKey::from_parts(kind, &value), Some(key));
    assert_eq!(RevocationKey::from_parts("signer", "short"), None);
    assert_eq!(RevocationKey::from_parts("unknown", "x"), None);
}

#[test]
fn with_keys_replaces_how_a_context_is_keyed() {
    use crate::CratestackContext;

    fn by_tenant(ctx: &CratestackContext) -> Vec<RevocationKey> {
        match ctx.auth_field("tenant") {
            Some(crate::Value::String(tenant)) => vec![RevocationKey::Custom(tenant.clone())],
            _ => Vec::new(),
        }
    }

    let revocations = StreamRevocations::default().with_keys(by_tenant);
    let ctx = CratestackContext::builder("u-1")
        .claim("tenant", crate::Value::String("acme".into()))
        .build();
    assert_eq!(
        revocations.keys_of(&ctx),
        vec![RevocationKey::Custom("acme".into())]
    );

    let watch = revocations.watch_context(&ctx).unwrap();
    assert_eq!(
        revocations.revoke(
            RevocationKey::Custom("acme".into()),
            StreamEndReason::PrincipalSuspended,
            None
        ),
        1
    );
    assert!(watch.try_ended().is_some());
}
