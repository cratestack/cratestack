//! `@device_auth` (cratestack#1303): a schema whose markers the generator
//! reads parses; the marker itself is checked here (spelling, placement).

use cratestack_core::schema::{DeviceAuthRole, procedure_device_auth};

use super::tests_support::{BASE, edit, parse, refused};

#[test]
fn the_reference_schema_parses_and_keeps_the_raw_markers() {
    let schema = parse(BASE).unwrap_or_else(|e| panic!("should parse: {e}"));
    let roles: Vec<_> = schema
        .procedures
        .iter()
        .filter_map(procedure_device_auth)
        .map(|parsed| parsed.role)
        .collect();
    assert_eq!(roles, DeviceAuthRole::ALL);
    // Kept as a raw attribute, as `@authn` is: the IR has no new field.
    let first = &schema.procedures[0].attributes[0];
    assert_eq!(first.raw, r#"@device_auth(role: "sendEnrolmentCode")"#);
}

#[test]
fn a_rest_schema_without_the_optional_leg_or_a_user_present_key_parses() {
    let no_code = BASE
        .replace("transport rpc\n", "")
        .replace("  @device_auth(role: \"sendEnrolmentCode\")\n", "");
    parse(&no_code).unwrap_or_else(|e| panic!("REST, no sendEnrolmentCode: {e}"));

    let no_user_present = BASE
        .replace("  userPresent Jwk?\n", "")
        .replace("  userPresent String?\n", "");
    parse(&no_user_present).unwrap_or_else(|e| panic!("no userPresent: {e}"));
}

#[test]
fn proof_names_the_field_that_carries_the_one_time_code() {
    let with_proof = BASE
        .replace(
            "  phone String\n  otp String\n  ambient Jwk",
            "  phone String\n  ambient Jwk",
        )
        .replace(
            "type ConfirmEnrolmentArgs {\n  enrolmentId String\n",
            "type ConfirmEnrolmentArgs {\n  enrolmentId String\n  otp String\n",
        )
        .replace(
            r#"@device_auth(role: "confirmEnrolment")"#,
            r#"@device_auth(role: "confirmEnrolment", proof: "otp")"#,
        );
    let schema = parse(&with_proof).unwrap_or_else(|e| panic!("proof: {e}"));
    let confirm = schema
        .procedures
        .iter()
        .find(|p| p.name == "confirmEnrolment");
    let parsed = procedure_device_auth(confirm.expect("present")).expect("marked");
    assert_eq!(parsed.proof.as_deref(), Some("otp"));
}

#[test]
fn a_proof_that_names_nothing_or_a_key_is_refused() {
    let marker = r#"@device_auth(role: "confirmEnrolment")"#;
    let missing = edit(
        marker,
        r#"@device_auth(role: "confirmEnrolment", proof: "otp")"#,
    );
    refused(
        &missing,
        "`proof: \"otp\"` names no field of its argument type `ConfirmEnrolmentArgs`",
    );
    let key = edit(
        marker,
        r#"@device_auth(role: "confirmEnrolment", proof: "ambient")"#,
    );
    refused(&key, "is a key's signature, not the one-time code");
    let wrong_role = edit(
        r#"@device_auth(role: "revoke")"#,
        r#"@device_auth(role: "revoke", proof: "deviceId")"#,
    );
    refused(
        &wrong_role,
        "takes `proof` only on `role: \"confirmEnrolment\"`",
    );
}

#[test]
fn a_malformed_marker_is_refused_where_it_is_written() {
    for (written, reason) in [
        (
            r#"@device_auth(role: "beginEnrolmen")"#,
            "has no role `beginEnrolmen`",
        ),
        (
            r#"@device_auth(role: "revokeDevice")"#,
            "has no role `revokeDevice`",
        ),
        (r#"@device_auth(kind: "revoke")"#, "has no argument `kind`"),
        ("@device_auth", "takes arguments in parentheses"),
        ("@device_auth()", "needs `role`"),
        (
            r#"@device_auth (role: "revoke")"#,
            "takes arguments in parentheses",
        ),
        (r#"@device_auth(role: revoke)"#, "must be a quoted string"),
        (
            r#"@device_auth(role: "revoke", role: "revoke")"#,
            "repeats `role`",
        ),
    ] {
        let source = edit(r#"@device_auth(role: "revoke")"#, written);
        let error = refused(&source, reason);
        // The marker is on `revokeDevice`, the last procedure.
        let last = source.rfind(written).expect("written");
        assert_eq!(
            error.line(),
            source[..last].matches('\n').count() + 1,
            "{written}"
        );
    }
}

#[test]
fn a_misspelled_name_is_pointed_at_the_right_one() {
    let source = edit(
        r#"@device_auth(role: "revoke")"#,
        r#"@deviceAuth(role: "revoke")"#,
    );
    refused(&source, "did you mean `@device_auth`?");
    let source = edit(
        r#"@device_auth(role: "revoke")"#,
        r#"@device_Auth(role: "revoke")"#,
    );
    refused(&source, "did you mean `@device_auth`?");
}

#[test]
fn two_markers_on_one_procedure_are_refused_at_the_second() {
    let source = edit(
        "  @device_auth(role: \"revoke\")\n",
        "  @device_auth(role: \"revoke\")\n  @device_auth(role: \"beginRenewal\")\n",
    );
    let error = refused(&source, "more than one @device_auth attribute");
    let second = source
        .rfind(r#"@device_auth(role: "beginRenewal")"#)
        .expect("second");
    assert_eq!(error.line(), source[..second].matches('\n').count() + 1);
}

#[test]
fn a_streaming_procedure_cannot_be_a_leg() {
    let source = edit(
        "  @device_auth(role: \"revoke\")\n",
        "  @device_auth(role: \"revoke\")\n  @stream\n",
    )
    .replace("): Revoked\n", "): Revoked[]\n");
    refused(&source, "cannot be both `@stream` and `@device_auth`");
}

#[test]
fn only_a_procedure_can_carry_the_marker() {
    let marker = r#"@device_auth(role: "revoke")"#;
    let base = BASE.replace("datasource db {\n  provider = \"none\"\n}\n", "");
    for (what, source) in [
        (
            "a model",
            format!("{base}\nmodel Note {{\n  id Int @id\n  {marker}\n}}\n"),
        ),
        (
            "a model block",
            format!("{base}\nmodel Note {{\n  id Int @id\n  @@device_auth(role: \"revoke\")\n}}\n"),
        ),
        (
            "a field",
            format!("{base}\nmodel Note {{\n  id Int @id {marker}\n}}\n"),
        ),
        (
            "a type field",
            format!("{base}\ntype Holder {{\n  id Int {marker}\n}}\n"),
        ),
        (
            "a subscription procedure",
            format!(
                "{base}\nmodel Note {{\n  id Int @id\n  @@allow(\"read\", true)\n}}\n\
                 subscription procedure watch(args: RevokeArgs): Revoked\n  {marker}\n  @allow(true)\n"
            ),
        ),
    ] {
        let error = parse(&source).expect_err(what);
        assert!(
            error.message().contains("device_auth"),
            "{what}: {}",
            error.message()
        );
    }
}
