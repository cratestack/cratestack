//! `DeviceKeyResolver` -> [`CoseVerifierResolver`].

use std::fmt;
use std::sync::Arc;

use cratestack_auth::DeviceKeyResolver;
use cratestack_core::CratestackError;

use crate::alg::CoseAlg;
use crate::keys::{CoseVerifierResolver, CoseVerifyKey};

/// Resolves COSE_Sign1 device requests through the service's
/// `cratestack_auth::DeviceKeyResolver` (ADR 0006 §8), by its required
/// `lookup_device_verifying_keys_by_thumbprint`.
///
/// What it maps, and why:
///
/// - **Only Ed25519 (`-19`).** Device keys are Ed25519 keys (the
///   resolver's type says so). For any other algorithm this returns no
///   candidate without asking the backend, so an ESP256 or HMAC header can
///   neither trigger a device-key lookup nor be verified with a device key.
/// - **Unknown or revoked is `Ok(vec![])`**, which the opener turns into
///   the coarse `401`, like every other verification failure (§10).
/// - **A backend `Err` is `CratestackError::Internal`**, so a `500`, and
///   its text never reaches the peer (`Internal` keeps it out of
///   `public_message()`). Mapping it to "no key" instead would make an
///   outage look like an attack in the logs, and look like an unknown key
///   to the client.
/// - **No `kid` filtering here.** The opener recomputes every candidate's
///   `kid` from the key and accepts only the key whose `kid` is the
///   header's (`open.rs`, step 4), so a registry that returns extra keys
///   for a prefix cannot let one device sign as another. Filtering here as
///   well would be a second copy of that check.
#[derive(Clone)]
pub struct DeviceKeyCoseResolver {
    devices: Arc<dyn DeviceKeyResolver>,
}

impl DeviceKeyCoseResolver {
    pub fn new(devices: Arc<dyn DeviceKeyResolver>) -> Self {
        Self { devices }
    }
}

impl fmt::Debug for DeviceKeyCoseResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceKeyCoseResolver")
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl CoseVerifierResolver for DeviceKeyCoseResolver {
    async fn resolve(
        &self,
        kid: &[u8],
        alg: CoseAlg,
    ) -> Result<Vec<CoseVerifyKey>, CratestackError> {
        if alg != CoseAlg::Ed25519 {
            return Ok(Vec::new());
        }
        let keys = self
            .devices
            .lookup_device_verifying_keys_by_thumbprint(kid)
            .await
            .map_err(|error| {
                CratestackError::Internal(format!("device key lookup failed: {error}"))
            })?;
        Ok(keys.into_iter().map(CoseVerifyKey::from_ed25519).collect())
    }
}
