//! The server and the client macros must compute one schema identity from
//! one file (cratestack#1065): it is the AAD `schema_sha` both ends of a
//! signed exchange rebuild, so a divergence is a 401 on every request.

mod server {
    cratestack::include_server_schema!("tests/fixtures/api_version.cstack", db = None);
}

mod client {
    cratestack::include_client_schema!("tests/fixtures/api_version.cstack");
}

#[test]
fn server_and_client_emit_the_same_identity() {
    assert_eq!(
        server::cratestack_schema::SCHEMA_SHA256_BYTES,
        client::cratestack_schema::SCHEMA_SHA256_BYTES
    );
    assert_eq!(
        server::cratestack_schema::SCHEMA_SHA256,
        client::cratestack_schema::SCHEMA_SHA256
    );
}
