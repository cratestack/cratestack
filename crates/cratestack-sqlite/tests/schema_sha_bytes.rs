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
