//! cratestack#1303: `--preset fespalier` emits a `DeviceAuthServer` over the
//! generated client for a schema whose procedures carry the device-auth
//! marker, and nothing else changes.
//!
//! Structural coverage (source-level, no Dart here). The snapshots
//! (`fespalier_device_auth_snapshot.rs`) pin the whole generated package;
//! the behaviour (request bodies, mapping, 403, `fieldErrors`, signed revoke,
//! abort isolation) runs under `just verify-dart-fespalier` from
//! `tests/fixtures/fespalier_device_auth_{rest,rpc}_test.dart`.

use std::collections::BTreeMap;

use cratestack_client_dart::{
    DartGeneratorConfig, DartGeneratorError, DartPreset, GeneratedDartPackage, generate_package,
};

const DEVICE_AUTH_FILE: &str = "lib/src/device_auth_server.dart";
const RPC: &str = include_str!("fixtures/device_auth_rpc.cstack");
const REST: &str = include_str!("fixtures/device_auth_rest.cstack");

fn without_markers(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("@device_auth"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn generate_from(source: &str, preset: DartPreset, git_ref: Option<&str>) -> GeneratedDartPackage {
    let schema = cratestack_parser::parse_schema_unvalidated(source).expect("fixture parses");
    let mut config = DartGeneratorConfig {
        library_name: "device_client".to_owned(),
        preset,
        ..DartGeneratorConfig::default()
    };
    if let Some(git_ref) = git_ref {
        config.fespalier_ref = git_ref.to_owned();
    }
    generate_package(&schema, &config).unwrap_or_else(|error| panic!("generates: {error}"))
}

fn file<'a>(package: &'a GeneratedDartPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("generated package should contain {name}"))
        .contents
        .as_str()
}

fn contents(package: &GeneratedDartPackage) -> BTreeMap<&str, &str> {
    package
        .files
        .iter()
        .map(|file| (file.file_name.as_str(), file.contents.as_str()))
        .collect()
}

#[test]
fn the_class_pubspec_and_export_exist_only_for_the_fespalier_preset_with_markers() {
    for source in [RPC, REST] {
        let marked = generate_from(source, DartPreset::Fespalier, None);
        assert!(contents(&marked).contains_key(DEVICE_AUTH_FILE));
        assert!(
            file(&marked, "lib/device_client.dart")
                .contains("export 'src/device_auth_server.dart';")
        );
        let pubspec = file(&marked, "pubspec.yaml");
        for package in [
            "fespalier_auth",
            "fespalier_keypair",
            "fespalier_sign_keypair",
        ] {
            assert!(
                pubspec.contains(&format!("  {package}:\n    git:")),
                "{package}"
            );
        }
        assert!(
            pubspec.contains("sdk: '>=3.12.0 <4.0.0'") && pubspec.contains("flutter: '>=3.44.0'")
        );

        let plain = generate_from(&without_markers(source), DartPreset::Fespalier, None);
        assert!(!contents(&plain).contains_key(DEVICE_AUTH_FILE));
        assert!(!file(&plain, "lib/device_client.dart").contains("device_auth"));
        let pubspec = file(&plain, "pubspec.yaml");
        assert!(
            !pubspec.contains("fespalier_sign_keypair")
                && pubspec.contains("sdk: '>=3.0.0 <4.0.0'")
        );
        assert!(!file(&plain, "README.md").contains("Device ceremonies"));
        assert!(file(&marked, "README.md").contains("### Device ceremonies"));
    }
}

#[test]
fn the_other_presets_ignore_the_marker_byte_for_byte() {
    for source in [RPC, REST] {
        for preset in [DartPreset::Default, DartPreset::Riverpod] {
            let marked = generate_from(source, preset, None);
            let plain = generate_from(&without_markers(source), preset, None);
            assert_eq!(contents(&marked), contents(&plain), "{preset:?}");
            assert!(!contents(&marked).contains_key(DEVICE_AUTH_FILE));
        }
    }
}

#[test]
fn fespalier_output_apart_from_the_class_is_what_it_is_without_the_marker() {
    for source in [RPC, REST] {
        let marked = generate_from(source, DartPreset::Fespalier, None);
        let plain = generate_from(&without_markers(source), DartPreset::Fespalier, None);
        let (marked, plain) = (contents(&marked), contents(&plain));
        for (name, text) in &plain {
            if ["pubspec.yaml", "README.md", "lib/device_client.dart"].contains(name) {
                continue;
            }
            assert_eq!(marked.get(name), Some(text), "{name}");
        }
    }
}

#[test]
fn every_fespalier_git_dependency_is_pinned_at_the_one_ref() {
    let package = generate_from(RPC, DartPreset::Fespalier, Some("v0.26.0"));
    let pubspec = file(&package, "pubspec.yaml");
    assert_eq!(pubspec.matches("ref: 'v0.26.0'").count(), 4, "{pubspec}");
    assert_eq!(
        pubspec
            .matches("url: https://github.com/fespalier/fespalier")
            .count(),
        4
    );
    assert!(!pubspec.contains("v0.21.0"));
    for path in [
        "fespalier_transport",
        "fespalier_auth",
        "fespalier_keypair",
        "fespalier_sign_keypair",
    ] {
        assert!(
            pubspec.contains(&format!("path: packages/{path}")),
            "{path}"
        );
    }
    assert!(file(&package, "README.md").contains("all at `v0.26.0`"));
    assert!(
        file(&package, DEVICE_AUTH_FILE).contains("pinned in this package's pubspec at `v0.26.0`")
    );
}

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

#[test]
fn user_fields_follow_the_convention_and_the_rest_go_to_claims() {
    let rpc = generate_from(RPC, DartPreset::Fespalier, None);
    let class = file(&rpc, DEVICE_AUTH_FILE);
    assert!(class.contains("id: answer.userId,") && class.contains("'phone': answer.phone,"));
    assert!(!class.contains("name: answer."));
    assert!(class.contains("final AuthUser Function(EnrolledDevice answer)? userOf;"));
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
    assert!(class.contains("TransportZone.run(neverAborted, call)"));
    assert!(class.contains("final neverAborted = Completer<void>().future;"));
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
