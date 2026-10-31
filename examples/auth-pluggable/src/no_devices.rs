//! A device registry with no device in it, for the in-house provider's COSE
//! half: this example authenticates COSE service signers only.

use cratestack_auth::{AuthError, DeviceKeyResolver};
use ed25519_dalek::VerifyingKey;

/// Knows no device key.
pub struct NoDevices;

#[async_trait::async_trait]
impl DeviceKeyResolver for NoDevices {
    async fn lookup_device_verifying_key(
        &self,
        _key_id: &str,
    ) -> Result<Option<VerifyingKey>, AuthError> {
        Ok(None)
    }

    async fn lookup_device_verifying_keys_by_thumbprint(
        &self,
        _kid_prefix: &[u8],
    ) -> Result<Vec<VerifyingKey>, AuthError> {
        Ok(Vec::new())
    }
}
