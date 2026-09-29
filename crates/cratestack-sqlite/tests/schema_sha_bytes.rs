//! `include_embedded_schema!` emits `SCHEMA_SHA256_BYTES` (cratestack#1006),
//! the same digest as the hex `SCHEMA_SHA256`, as the bytes a signed
//! request's binding carries.

cratestack::include_embedded_schema!("tests/fixtures/builder_pattern.cstack");

#[test]
fn the_bytes_are_the_hex_digest() {
    let hex: String = cratestack_schema::SCHEMA_SHA256_BYTES
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(hex, cratestack_schema::SCHEMA_SHA256);
}

mod golden {
    cratestack::include_embedded_schema!("tests/fixtures/identity_golden.cstack");
}

/// The identity of the one-model fixture (the IR of `model Widget { id Int @id }`), pinned by
/// `cratestack-core`'s own golden test and the CLI's (cratestack#1065).
#[test]
fn the_identity_is_the_canonical_golden_digest() {
    assert_eq!(
        golden::cratestack_schema::SCHEMA_SHA256,
        "95c11ca292e854994d452dcc0d88c7de6ab309b0422fc1e30ab46e56a7757a5f"
    );
}
