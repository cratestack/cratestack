use super::*;
use crate::schema::SourceSpan;

fn attribute(raw: &str) -> Attribute {
    Attribute {
        raw: raw.to_owned(),
        span: SourceSpan {
            start: 0,
            end: raw.len(),
            line: 1,
        },
    }
}

#[test]
fn every_role_is_read_by_its_interface_method_name() {
    for role in DeviceAuthRole::ALL {
        let raw = format!("@device_auth(role: \"{}\")", role.name());
        let parsed = parse_device_auth_attribute(&raw).expect("valid");
        assert_eq!(parsed, ParsedDeviceAuth { role, proof: None });
    }
    assert_eq!(DeviceAuthRole::REQUIRED.len(), 5);
    assert!(!DeviceAuthRole::SendEnrolmentCode.is_required());
    assert!(
        DeviceAuthRole::REQUIRED
            .iter()
            .all(|role| role.is_required())
    );
    assert!(!DeviceAuthRole::Revoke.is_public());
    assert!(DeviceAuthRole::BeginRenewal.is_public());
}

#[test]
fn proof_names_a_field_on_confirm_enrolment() {
    let parsed =
        parse_device_auth_attribute(r#"@device_auth(role: "confirmEnrolment", proof: 'otp')"#)
            .expect("valid");
    assert_eq!(parsed.role, DeviceAuthRole::ConfirmEnrolment);
    assert_eq!(parsed.proof.as_deref(), Some("otp"));
    let reversed =
        parse_device_auth_attribute(r#"@device_auth( proof : "otp" , role: "confirmEnrolment" )"#)
            .expect("order does not matter");
    assert_eq!(reversed, parsed);
}

#[test]
fn malformed_attributes_are_refused_with_a_reason() {
    for (raw, reason) in [
        ("@device_auth", "takes arguments in parentheses"),
        ("@device_auth()", "needs `role`"),
        (r#"@device_auth(proof: "otp")"#, "needs `role`"),
        (r#"@device_auth(role: "begin")"#, "has no role `begin`"),
        (
            "@device_auth(role: beginEnrolment)",
            "must be a quoted string",
        ),
        (r#"@device_auth(kind: "revoke")"#, "has no argument `kind`"),
        (
            r#"@device_auth(role: "revoke", role: "revoke")"#,
            "repeats `role`",
        ),
        (r#"@device_auth(role "revoke")"#, "is not `key:"),
        (r#"@device_auth(role: "revoke",)"#, "empty argument"),
        (r#"@device_auth(role: "revoke)"#, "unclosed quote"),
        (
            r#"@device_auth(role: "beginEnrolment", proof: "otp")"#,
            "only on `role: \"confirmEnrolment\"`",
        ),
        (
            r#"@device_auth(role: "confirmEnrolment", proof: "o-tp")"#,
            "must name a field",
        ),
        (
            r#"@device_auth(role: "confirmEnrolment", proof: "")"#,
            "must name a field",
        ),
    ] {
        let error = parse_device_auth_attribute(raw).expect_err(raw);
        assert!(error.contains(reason), "{raw}: {error}");
    }
}

#[test]
fn the_role_list_is_in_the_unknown_role_error() {
    let error = parse_device_auth_attribute(r#"@device_auth(role: "x")"#).unwrap_err();
    for role in DeviceAuthRole::ALL {
        assert!(error.contains(role.name()), "{error}");
    }
}

#[test]
fn only_the_exact_name_is_a_device_auth_attribute() {
    let attributes = vec![
        attribute("@device_authx(role: \"revoke\")"),
        attribute("@allow(true)"),
        attribute(r#"@device_auth(role: "revoke")"#),
    ];
    let found = device_auth_attribute(&attributes).expect("the third");
    assert_eq!(found.raw, r#"@device_auth(role: "revoke")"#);
    assert!(device_auth_attribute(&attributes[..2]).is_none());
}
