//! Verifying-key base64url round-trip.

use crate::signed_request::{decode_verifying_key, encode_verifying_key};

#[test]
fn round_trips_verifying_keys() {
    let verifying_key = super::example_signing_key().verifying_key();
    let encoded = encode_verifying_key(&verifying_key);
    let decoded = decode_verifying_key(&encoded).expect("verifying key should decode");

    assert_eq!(decoded, verifying_key);
}
