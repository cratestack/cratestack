//! Tests for `signed_request`, split by topic into sibling submodules to
//! stay under the 200-LoC budget.

mod canonical;
mod device_pop;
mod jwks_rotation;
mod keys;
mod nonce_store;
mod verify_basic;

use ed25519_dalek::SigningKey;

/// A fixed, arbitrary Ed25519 seed used across tests that just need *a*
/// deterministic signing key, not any particular one.
pub(super) fn example_signing_key() -> SigningKey {
    SigningKey::from_bytes(&[
        0x52, 0x21, 0x09, 0x7a, 0x8c, 0x1b, 0x2d, 0x48, 0x93, 0x4f, 0x61, 0xf0, 0xa5, 0x33, 0x1e,
        0x9c, 0x74, 0x08, 0xa1, 0x64, 0x5b, 0x91, 0x2f, 0x3c, 0xb8, 0x27, 0xa0, 0xd9, 0x1f, 0x45,
        0x6c, 0x22,
    ])
}

pub(super) fn example_key_id() -> String {
    "vk_example".to_string()
}
