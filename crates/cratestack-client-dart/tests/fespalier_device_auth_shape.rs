//! cratestack#1303: the structure of the generated `DeviceAuthServer`: its
//! paths, wrappers, and the optional pieces (`userPresent`, `proof:`).

use cratestack_client_dart::DartPreset;

mod fespalier_device_auth_common;
use fespalier_device_auth_common::*;

#[test]
fn the_unsigned_paths_are_the_servers_for_each_transport() {
    let rpc = generate_from(RPC, DartPreset::Fespalier, None);
    let class = file(&rpc, DEVICE_AUTH_FILE);
    for op in [
        "sendEnrolmentCode",
        "beginEnrolment",
        "confirmEnrolment",
        "beginRenewal",
        "confirmRenewal",
    ] {
        assert!(
            class.contains(&format!("'$prefix/rpc/procedure.{op}',")),
            "{op}"
        );
    }
    assert!(
        !class.contains("procedure.revokeDevice"),
        "revoke is signed, not unsigned"
    );
    assert!(class.contains("static List<String> unsignedPaths(Uri baseUri) {"));

    let rest = generate_from(REST, DartPreset::Fespalier, None);
    let class = file(&rest, DEVICE_AUTH_FILE);
    for op in [
        "beginEnrolment",
        "confirmEnrolment",
        "beginRenewal",
        "confirmRenewal",
    ] {
        assert!(
            class.contains(&format!("'$prefix$basePath/\\$procs/{op}',")),
            "{op}"
        );
    }
    assert!(
        !class.contains("sendEnrolmentCode"),
        "no marker for it in the REST fixture"
    );
    assert!(class.contains("String basePath = '/api',"));
    assert!(
        class.contains("CratestackHttpAdapter(")
            && class.contains("on CratestackRestException catch")
    );
    assert!(rpc_only_names_absent(class));
}

fn rpc_only_names_absent(class: &str) -> bool {
    !class.contains("CratestackRpc")
}

#[test]
fn wrapper_classes_come_from_the_generators_own_naming_and_exist() {
    // The reference schema's `BeginEnrolmentArgs` type takes the plain
    // wrapper name, so the wrapper is `BeginEnrolmentProcedureArgs`.
    let package = generate_from(RPC, DartPreset::Fespalier, None);
    let class = file(&package, DEVICE_AUTH_FILE);
    let procedures = file(&package, "lib/src/procedures.dart");
    for (wrapper, procedure) in [
        ("SendEnrolmentCodeArgs", "sendEnrolmentCode"),
        ("BeginEnrolmentProcedureArgs", "beginEnrolment"),
        ("ConfirmEnrolmentProcedureArgs", "confirmEnrolment"),
        ("BeginRenewalProcedureArgs", "beginRenewal"),
        ("ConfirmRenewalProcedureArgs", "confirmRenewal"),
        ("RevokeDeviceArgs", "revokeDevice"),
    ] {
        assert!(
            class.contains(&format!("{wrapper}(")) || class.contains(&format!("({wrapper} ")),
            "{wrapper}"
        );
        assert!(
            procedures.contains(&format!("class {wrapper} ")),
            "{wrapper} is declared"
        );
        assert!(class.contains(&format!(".{procedure}(")), "{procedure}");
    }
}

#[test]
fn a_schema_without_a_user_present_key_has_no_user_present_code() {
    let source = RPC
        .replace("  userPresent Jwk?\n", "")
        .replace("  userPresent String?\n", "");
    let package = generate_from(&source, DartPreset::Fespalier, None);
    let class = file(&package, DEVICE_AUTH_FILE);
    assert!(!class.contains("userPresent"), "{class}");
    assert!(class.contains("KeyRole.ambient: answer.ambient,"));
}

#[test]
fn the_proof_field_is_read_only_when_the_schema_names_one() {
    let with = generate_from(REST, DartPreset::Fespalier, None);
    assert!(file(&with, DEVICE_AUTH_FILE).contains("otp: '${proof ?? ''}',"));
    let without = generate_from(RPC, DartPreset::Fespalier, None);
    let class = file(&without, DEVICE_AUTH_FILE);
    assert!(
        !class.contains("${proof"),
        "the proof is ignored without `proof:`"
    );
    assert!(
        class.contains("otp: '${start.fields['otp'] ?? ''}',"),
        "the code goes in leg one"
    );
}
