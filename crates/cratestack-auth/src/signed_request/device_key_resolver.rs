//! Device-key resolution: the last-resort resolver tier consulted before
//! falling back to id-token cnf-bound proof-of-possession.

use async_trait::async_trait;
use ed25519_dalek::VerifyingKey;

use crate::AuthError;

/// Resolves a device's ed25519 verifying key by its key id.
///
/// Device-signed requests carry `keyId=<device-key-id>`, which is not in
/// any service JWKS or the static trusted-keys map, so the verifier falls
/// through to this resolver. The service that owns the device-key registry
/// (auth-service) plugs in a DB-backed implementation, giving device
/// requests true proof-of-possession: the transport signature is verified
/// against the stored public key.
#[async_trait]
pub trait DeviceKeyResolver: Send + Sync {
    /// Return the active device key's verifying key, or `None` when the kid
    /// is unknown or revoked. `Err` is reserved for backend failures (e.g.
    /// the store being unreachable) so the caller can tell "no such key"
    /// apart from "couldn't check".
    async fn lookup_device_verifying_key(
        &self,
        key_id: &str,
    ) -> Result<Option<VerifyingKey>, AuthError>;
}
