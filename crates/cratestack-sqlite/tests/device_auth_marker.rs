//! cratestack#1303: `include_embedded_schema!` ignores the device-auth marker.
//! The schema is the one `cratestack-api/tests/device_auth_marker.rs` serves
//! and a client reads; here it only has to compile, with its six marked
//! procedures declared.

mod schema {
    cratestack::include_embedded_schema!("tests/fixtures/device_auth.cstack");
}

#[test]
fn the_embedded_schema_with_the_marker_compiles_and_declares_the_types() {
    let device = schema::cratestack_schema::RevokeArgs {
        deviceId: "dev-1".to_owned(),
    };
    assert_eq!(device.deviceId, "dev-1");
}
