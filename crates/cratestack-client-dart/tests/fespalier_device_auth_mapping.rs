//! cratestack#1303: how the generated `DeviceAuthServer` maps users, 403s
//! and a schema that skipped the parser.

use cratestack_client_dart::{
    DartGeneratorConfig, DartGeneratorError, DartPreset, generate_package,
};

mod fespalier_device_auth_common;
use fespalier_device_auth_common::*;

#[test]
fn user_fields_follow_the_convention_and_the_rest_go_to_claims() {
    let rpc = generate_from(RPC, DartPreset::Fespalier, None);
    let class = file(&rpc, DEVICE_AUTH_FILE);
    assert!(class.contains("id: answer.userId,") && class.contains("'phone': answer.phone,"));
    assert!(!class.contains("name: answer."));
    assert!(class.contains("final fauth.AuthUser Function(EnrolledDevice answer)? userOf;"));
    assert!(class.contains("userOf?.call(answer) ??"));
    assert!(class.contains("final Object? Function(Object error)? fieldErrors;"));

    let rest = generate_from(REST, DartPreset::Fespalier, None);
    let class = file(&rest, DEVICE_AUTH_FILE);
    assert!(class.contains("name: answer.name,") && class.contains("email: answer.email,"));
    assert!(class.contains("'phone': answer.phone,"));
}

#[test]
fn a_403_is_rejected_on_the_legs_the_interface_says_and_the_call_runs_in_the_isolating_zone() {
    let package = generate_from(RPC, DartPreset::Fespalier, None);
    let class = file(&package, DEVICE_AUTH_FILE);
    assert_eq!(
        class.matches("rejectedOn403: true,").count(),
        3,
        "confirmEnrolment and both renewals"
    );
    assert!(class.contains("ftransport.TransportZone.run(neverAborted, call)"));
    assert!(class.contains("final neverAborted = async.Completer<void>().future;"));
    assert!(class.contains("suspected"), "documented as a workaround");
    // The revoke call goes through the signing transport, never the plain one.
    assert!(class.contains("_proceduresOver(_SignedBy(transport, credential, signer))"));
    assert!(class.contains("signer.signFor(credential, request)"));
}

#[test]
fn a_schema_that_skipped_the_parser_is_an_error_not_a_panic() {
    let partial = RPC
        .lines()
        .filter(|line| !line.contains(r#"role: "revoke""#))
        .collect::<Vec<_>>()
        .join("\n");
    let schema = cratestack_parser::parse_schema_unvalidated(&partial).expect("parses");
    let error = generate_package(
        &schema,
        &DartGeneratorConfig {
            preset: DartPreset::Fespalier,
            ..DartGeneratorConfig::default()
        },
    )
    .expect_err("a marker set without `revoke` has no DeviceAuthServer");
    assert!(
        matches!(&error, DartGeneratorError::DeviceAuthSchema(why) if why.contains("`revoke`")),
        "{error}"
    );

    let mut malformed = cratestack_parser::parse_schema_unvalidated(RPC).expect("parses");
    malformed
        .types
        .iter_mut()
        .find(|decl| decl.name == "RevokeArgs")
        .expect("type")
        .fields
        .clear();
    let error = generate_package(
        &malformed,
        &DartGeneratorConfig {
            preset: DartPreset::Fespalier,
            ..DartGeneratorConfig::default()
        },
    )
    .expect_err("a revoke without `deviceId`");
    assert!(
        matches!(error, DartGeneratorError::DeviceAuthSchema(_)),
        "{error}"
    );
}
