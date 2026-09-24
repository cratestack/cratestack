//! Keys: the signing and resolution traits, verification keys, and the
//! in-process implementations.

mod hmac;
mod key_provider;
mod signers;
mod static_resolver;
mod traits;
mod verify;
mod verify_key;

pub use hmac::{HmacSecret, HmacSigner, MIN_HMAC_SECRET_LEN};
pub use key_provider::KeyProviderMacKeys;
pub use signers::{Ed25519Signer, P256Signer};
pub use static_resolver::StaticVerifierResolver;
pub use traits::{CoseSigner, CoseVerifierResolver};
pub use verify_key::CoseVerifyKey;

pub(crate) use verify::esp256_low_s;
