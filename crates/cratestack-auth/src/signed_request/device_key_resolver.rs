//! Device-key resolution: the last-resort resolver tier consulted before
//! falling back to id-token cnf-bound proof-of-possession, and the lookup
//! COSE-signed device requests resolve through.

use async_trait::async_trait;
use ed25519_dalek::VerifyingKey;

use crate::AuthError;

/// Resolves a device's ed25519 verifying key, by its key id or by its
/// COSE thumbprint prefix.
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

    /// Return every active device key whose RFC 9679 COSE Key Thumbprint
    /// starts with `kid_prefix`, or an empty `Vec` when there is none
    /// (unknown or revoked). `Err` is for backend failures only, as above.
    ///
    /// A COSE_Sign1 device request (ADR 0006 §3, §8) names its key by the
    /// first 8 bytes of that thumbprint, not by the `keyId` string, so the
    /// registry needs this second index. `cratestack-cose` exposes the
    /// thumbprint as `cratestack_cose::thumbprint::okp_ed25519_thumbprint`
    /// (SHA-256 of the deterministic CBOR `{1: 1, -1: 6, -2: x}`); store it,
    /// or its 8-byte prefix, next to each key when it is enrolled.
    ///
    /// Several keys may share a prefix (8 bytes collide at about 2³² keys),
    /// so return all of them. Returning too many is safe: the COSE opener
    /// recomputes each candidate's thumbprint and accepts only a key whose
    /// own `kid` is the message's.
    ///
    /// **Required, with no default**, on purpose (ADR 0006, "Decisions taken
    /// while scoping P0"; cratestack#1005): a provided `Ok(vec![])` would
    /// let an existing implementation compile and then silently refuse
    /// every COSE device request. Implementors that serve no COSE traffic
    /// return `Ok(Vec::new())` explicitly.
    async fn lookup_device_verifying_keys_by_thumbprint(
        &self,
        kid_prefix: &[u8],
    ) -> Result<Vec<VerifyingKey>, AuthError>;
}
