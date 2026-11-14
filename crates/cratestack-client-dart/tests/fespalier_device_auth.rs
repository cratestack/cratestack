//! cratestack#1303: `--preset fespalier` emits a `DeviceAuthServer` over the
//! generated client for a schema whose procedures carry the device-auth
//! marker, and nothing else changes.
//!
//! Structural coverage (source-level, no Dart here). The snapshots
//! (`fespalier_device_auth_snapshot.rs`) pin the whole generated package;
//! the behaviour (request bodies, mapping, 403, `fieldErrors`, signed revoke,
//! abort isolation) runs under `just verify-dart-fespalier` from
//! `tests/fixtures/fespalier_device_auth_{rest,rpc}_test.dart`.

use cratestack_client_dart::DartPreset;

mod fespalier_device_auth_common;
use fespalier_device_auth_common::*;

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
