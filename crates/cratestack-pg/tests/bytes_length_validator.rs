//! cratestack#572 regression test, server-schema (`include_server_schema!`)
//! counterpart of `cratestack-sqlite/tests/bytes_length_validator.rs` — see
//! that file's module doc for the full mechanism.
//!
//! `model/inputs.rs::generate_create_input_struct` is the exact same
//! function `include/server/collect/models.rs` and `include/embedded.rs`
//! both call, so the fix in `cratestack-macros/src/validators/emit.rs` is
//! a single shared fix, but this test proves it concretely on the server
//! path too rather than relying on that being obvious from reading the
//! code.
//!
//! Deliberately DB-free: `validate()` is a pure function on the generated
//! input struct, so this test needs no live/testcontainer Postgres and
//! never skips.

use cratestack::CreateModelInput;
use cratestack::include_server_schema;

include_server_schema!(
    "tests/fixtures/bytes_length_validator.cstack",
    db = Postgres
);

use cratestack_schema::CreateBlobInput;

#[test]
fn digest_shorter_than_min_is_rejected() {
    let input = CreateBlobInput {
        id: 1,
        digest: vec![0u8; 31],
    };
    assert!(input.validate().is_err());
}

#[test]
fn digest_longer_than_max_is_rejected() {
    let input = CreateBlobInput {
        id: 2,
        digest: vec![0u8; 33],
    };
    assert!(input.validate().is_err());
}

#[test]
fn digest_exactly_at_bound_is_accepted() {
    let input = CreateBlobInput {
        id: 3,
        digest: vec![0u8; 32],
    };
    assert!(input.validate().is_ok());
}
