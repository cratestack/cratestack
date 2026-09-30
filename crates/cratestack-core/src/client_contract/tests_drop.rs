//! One check per drop-list entry: adding it to a field, a model or a
//! procedure leaves every op digest where it was.

use super::tests::{attr, sample};
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
fn every_dropped_attribute_is_wire_neutral() {
    let base = sample();
    let table = op_contract_digests(&base);
    for (name, reason) in DROPPED_ATTRIBUTES {
        assert!(!reason.is_empty(), "{name} needs a reason");
        let raw = format!("{name}(x)");
        let mut schema = base.clone();
        schema.models[0].attributes.push(attr(&raw));
        schema.models[0].fields[1].attributes.push(attr(&raw));
        schema.types[0].fields[0].attributes.push(attr(&raw));
        schema.procedures[0].attributes.push(attr(&raw));
        assert_eq!(op_contract_digests(&schema), table, "{name} moved a digest");
    }
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
