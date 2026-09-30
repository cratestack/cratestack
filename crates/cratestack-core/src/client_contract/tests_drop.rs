//! Name matching and the projections' structural rules. Per-entry
//! invariance over parsed source is `cratestack-parser/tests/op_contract_dropped.rs`;
//! who reads each name is pinned by `tests_readers.rs`.

use super::attrs::{DROPPED_ATTRIBUTES, attribute_name, is_dropped};
use super::tests::sample;
use super::*;

#[test]
fn attribute_names_are_exact() {
    assert_eq!(attribute_name("@@allow(\"read\", x)"), "@@allow");
    assert_eq!(attribute_name("@length(min: 1)"), "@length");
    assert_eq!(attribute_name("@server_only"), "@server_only");
    assert!(is_dropped("@@index([a])"));
    assert!(
        !is_dropped("@Deny(x)"),
        "a case variant is unknown, so it stays in"
    );
    assert!(!is_dropped("@allow2(x)"));
}

#[test]
fn a_server_only_field_is_on_no_contract() {
    let base = sample();
    let mut schema = base.clone();
    schema.models[0]
        .fields
        .push(super::tests::field("secret", "String", &["@server_only"]));
    assert_eq!(op_contract_digests(&schema), op_contract_digests(&base));
}

#[test]
fn an_unrelated_new_model_procedure_or_type_moves_no_existing_digest() {
    let base = sample();
    let before = op_contract_digests(&base);
    let mut schema = base.clone();
    schema.models.push(super::tests::model(
        "Other",
        vec![super::tests::field("id", "Int", &["@id"])],
        &[],
    ));
    schema
        .procedures
        .push(super::tests::procedure("pong", "Other", "Other", &[]));
    let after = op_contract_digests(&schema);
    for (key, digest) in &before {
        let now = after.iter().find(|(k, _)| k == key).unwrap();
        assert_eq!(&now.1, digest, "{key}");
    }
    assert!(after.len() > before.len());
}

#[test]
fn every_dropped_attribute_has_a_reason_and_a_unique_name() {
    let mut names: Vec<_> = DROPPED_ATTRIBUTES.iter().map(|(n, _)| *n).collect();
    assert!(
        DROPPED_ATTRIBUTES
            .iter()
            .all(|(_, reason)| !reason.is_empty())
    );
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), DROPPED_ATTRIBUTES.len());
}

#[test]
fn attributes_with_no_semantics_today_are_not_pre_approved() {
    for raw in ["@@map(\"t\")", "@map(\"c\")", "@@mcp(x)", "@mcp(x)"] {
        assert!(!is_dropped(raw), "{raw} must stay in until it is reviewed");
    }
}
