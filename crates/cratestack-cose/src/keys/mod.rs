//! Keys: the signing and resolution traits, typed verification keys, and
//! the in-process implementations.

mod hmac;
mod signers;
mod static_resolver;
mod traits;
mod verify_key;

pub use hmac::{HmacSecret, HmacSigner, MIN_HMAC_SECRET_LEN};
pub use signers::{Ed25519Signer, P256Signer};
pub use static_resolver::StaticVerifierResolver;
pub use traits::{CoseSigner, CoseVerifierResolver};
pub use verify_key::CoseVerifyKey;
