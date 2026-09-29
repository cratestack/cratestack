//! The schema digest is an identity of the schema's meaning, not of its
//! source text (cratestack#1065). It is bound into every signed request's
//! AAD, so two builds of the same contract must agree on it — however the
//! `.cstack` file was commented, indented or spaced — and two different
//! contracts must not.

mod base {
    cratestack::include_client_schema!("tests/fixtures/identity_base.cstack");
}
mod comment {
    cratestack::include_client_schema!("tests/fixtures/identity_comment.cstack");
}
mod whitespace {
    cratestack::include_client_schema!("tests/fixtures/identity_ws.cstack");
}
mod renamed {
    cratestack::include_client_schema!("tests/fixtures/identity_rename.cstack");
}
mod retyped {
    cratestack::include_client_schema!("tests/fixtures/identity_type.cstack");
}

#[test]
fn a_comment_only_edit_keeps_the_identity() {
    assert_eq!(
        base::cratestack_schema::SCHEMA_SHA256_BYTES,
        comment::cratestack_schema::SCHEMA_SHA256_BYTES
    );
}

#[test]
fn a_whitespace_only_edit_keeps_the_identity() {
    assert_eq!(
        base::cratestack_schema::SCHEMA_SHA256_BYTES,
        whitespace::cratestack_schema::SCHEMA_SHA256_BYTES
    );
}

#[test]
fn a_renamed_field_changes_the_identity() {
    assert_ne!(
        base::cratestack_schema::SCHEMA_SHA256_BYTES,
        renamed::cratestack_schema::SCHEMA_SHA256_BYTES
    );
}

#[test]
fn a_retyped_field_changes_the_identity() {
    assert_ne!(
        base::cratestack_schema::SCHEMA_SHA256_BYTES,
        retyped::cratestack_schema::SCHEMA_SHA256_BYTES
    );
}
