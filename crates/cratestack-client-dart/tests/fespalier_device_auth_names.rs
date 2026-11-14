//! cratestack#1303: names that clash, and the ref a schema with markers needs.

use cratestack_client_dart::{DartGeneratorError, DartPreset};

mod fespalier_device_auth_common;
use fespalier_device_auth_common::*;

/// Schema types named like the fespalier types the class imports.
const CLASHING: [(&str, &str); 4] = [
    ("RenewalChallenge", "RoleChallenge"),
    ("EnrolledDevice", "DeviceEnrolment"),
    ("EnrolmentChallenges", "EnrolmentChallenge"),
    ("BeginEnrolmentArgs", "EnrolmentStart"),
];

fn renamed(source: &str) -> String {
    CLASHING
        .iter()
        .fold(source.to_owned(), |text, (from, to)| text.replace(from, to))
}

#[test]
fn a_schema_type_named_like_a_fespalier_type_is_not_an_ambiguous_import() {
    for source in [RPC, REST] {
        let package = generate_from(&renamed(source), DartPreset::Fespalier, None);
        let class = file(&package, DEVICE_AUTH_FILE);
        for (prefix, library) in [
            ("fsign", "fespalier_sign_keypair"),
            ("fauth", "fespalier_auth"),
            ("fkey", "fespalier_keypair"),
            ("ftransport", "fespalier_transport"),
        ] {
            let import = format!("package:{library}/{library}.dart' as {prefix}");
            assert!(class.contains(&import), "{import}");
        }
        // The schema's own `RoleChallenge` is the one the generated client
        // returns; fespalier's is only ever named through its prefix.
        assert!(class.contains("Future<fsign.RoleChallenge> beginRenewal("));
        assert!(class.contains("implements fsign.DeviceAuthServer"));
        assert!(class.contains("final fauth.AuthUser Function(DeviceEnrolment answer)? userOf;"));
        // No fespalier library is imported unprefixed, and the schema's own
        // types are the only unqualified ones.
        for line in class
            .lines()
            .filter(|line| line.starts_with("import 'package:fespalier"))
        {
            assert!(line.contains(" as f"), "{line}");
        }
        for qualified in [
            "fsign.EnrolmentChallenge",
            "fsign.EnrolmentStart",
            "fsign.DeviceEnrolment",
            "fsign.Attestation.parse(",
            "fauth.AuthRejected()",
            "fkey.KeyRole",
            "ftransport.HttpTransport",
        ] {
            assert!(class.contains(qualified), "{qualified}");
        }
        assert!(
            !class.contains(" EnrolmentChallenge("),
            "bare fespalier call"
        );
    }
}

#[test]
fn with_markers_the_fespalier_ref_must_be_a_release_tag() {
    for git_ref in [
        "main",
        "0123abcdef0123abcdef0123abcdef0123abcdef",
        "v0.21",
        "0.21.0",
        "v0.21.0-rc1",
    ] {
        for source in [RPC, REST] {
            let error = try_generate(source, DartPreset::Fespalier, Some(git_ref))
                .expect_err("only a tag resolves fespalier_sign_keypair's siblings");
            assert!(
                matches!(&error, DartGeneratorError::FespalierRefNotATag(found) if found == git_ref),
                "{git_ref}: {error}"
            );
            assert!(error.to_string().contains("vMAJOR.MINOR.PATCH"), "{error}");
        }
    }
    for git_ref in ["v0.21.0", "v0.26.0", "v1.20.300"] {
        try_generate(RPC, DartPreset::Fespalier, Some(git_ref)).expect("a release tag");
    }
}

#[test]
fn without_markers_any_fespalier_ref_is_still_accepted() {
    let plain = without_markers(RPC);
    for git_ref in ["main", "0123abc"] {
        try_generate(&plain, DartPreset::Fespalier, Some(git_ref))
            .expect("no marker, no sign_keypair");
    }
}
