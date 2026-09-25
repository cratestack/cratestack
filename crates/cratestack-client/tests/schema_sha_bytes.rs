//! `include_client_schema!` emits `SCHEMA_SHA256_BYTES` (cratestack#1006):
//! the digest a signed request's binding carries, the same value as the hex
//! `SCHEMA_SHA256`. The Rust client (#1007) seals with it, and it must equal
//! what the server's module emits for the same file.

mod schema {
    cratestack::include_client_schema!("tests/fixtures/builder_pattern.cstack");
}

#[test]
fn the_bytes_are_the_hex_digest() {
    let hex: String = schema::cratestack_schema::SCHEMA_SHA256_BYTES
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(hex, schema::cratestack_schema::SCHEMA_SHA256);
}
