//! `@device_auth` (cratestack#1303) and the two digests.
//!
//! The marker makes a Dart helper over ops that already exist: no wire type,
//! route, op id or decode changes. So it is out of every per-op contract
//! digest (which is bound into the COSE AAD, where a moved digest is a 401 at
//! deploy for no wire change) and in the whole-schema identity (it is part of
//! the schema's meaning, and that digest is only a warn-only header).

use std::collections::BTreeMap;

use cratestack_core::{digest_hex, op_contract_digests, schema_digest_hex};

const MARKED: &str = include_str!("fixtures/device_auth.cstack");

fn unmarked() -> String {
    MARKED
        .lines()
        .filter(|line| !line.trim_start().starts_with("@device_auth"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn digests(source: &str) -> BTreeMap<String, String> {
    let schema = cratestack_parser::parse_schema(source).expect("schema parses");
    op_contract_digests(&schema)
        .into_iter()
        .map(|(key, digest)| (key, digest_hex(&digest)))
        .collect()
}

fn identity(source: &str) -> String {
    // Unvalidated: the roles are swapped below without regard to their shapes.
    schema_digest_hex(&cratestack_parser::parse_schema_unvalidated(source).expect("parses"))
}

#[test]
fn the_fixture_has_all_six_markers_and_the_unmarked_copy_has_none() {
    assert_eq!(MARKED.matches("@device_auth(").count(), 6);
    assert_eq!(unmarked().matches("@device_auth").count(), 0);
    for op in [
        "procedure.sendEnrolmentCode",
        "procedure.beginEnrolment",
        "procedure.confirmEnrolment",
        "procedure.beginRenewal",
        "procedure.confirmRenewal",
        "procedure.revokeDevice",
    ] {
        assert!(digests(MARKED).contains_key(op), "{op}");
    }
}

#[test]
fn the_markers_move_no_op_contract_digest() {
    assert_eq!(digests(MARKED), digests(&unmarked()));
}

#[test]
fn each_marker_alone_moves_no_op_contract_digest() {
    // Drop the other five, one at a time: no single role is the exception.
    let plain = digests(&unmarked());
    for keep in MARKED
        .lines()
        .filter(|l| l.trim_start().starts_with("@device_auth"))
    {
        let source = MARKED
            .lines()
            .filter(|l| !l.trim_start().starts_with("@device_auth") || *l == keep)
            .collect::<Vec<_>>()
            .join("\n");
        // A lone marker is refused by the all-or-nothing rule; read it
        // without validation, as the digest does not need the rule to hold.
        let schema = cratestack_parser::parse_schema_unvalidated(&source).expect("parses");
        let table: BTreeMap<String, String> = op_contract_digests(&schema)
            .into_iter()
            .map(|(key, digest)| (key, digest_hex(&digest)))
            .collect();
        assert_eq!(table, plain, "{keep}");
    }
}

#[test]
fn the_markers_move_the_schema_identity() {
    assert_ne!(identity(MARKED), identity(&unmarked()));
}

#[test]
fn a_changed_role_or_proof_moves_the_schema_identity() {
    let swapped = MARKED
        .replace(r#"role: "beginRenewal""#, r#"role: "TMP""#)
        .replace(r#"role: "confirmRenewal""#, r#"role: "beginRenewal""#)
        .replace(r#"role: "TMP""#, r#"role: "confirmRenewal""#);
    assert_ne!(identity(&swapped), identity(MARKED));
    assert_eq!(identity(MARKED), identity(MARKED), "stable");
}
