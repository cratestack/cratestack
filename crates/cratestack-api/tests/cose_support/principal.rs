//! Shared by `cose_auth_provider_rest.rs` and `cose_auth_provider_rpc.rs`
//! (cratestack#1077): the `CoseAuthProvider` a generated router takes as its
//! `auth`, over a device registry that is a `RegistryVerifierResolver`, the
//! same type the envelope verifies with.

use std::sync::Arc;

use cratestack::cose::auth::CoseAuthProvider;
use cratestack::cose::{CoseAlg, CoseVerifierResolver, RegistryVerifierResolver};
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_auth::{AuthError, DeviceKeyResolver};
use ed25519_dalek::VerifyingKey;

/// The device key registry the adapter reads. It is handed to the envelope
/// as well when a test wants one registry to serve both.
pub struct DeviceRegistry(pub Arc<RegistryVerifierResolver>);

#[async_trait::async_trait]
impl DeviceKeyResolver for DeviceRegistry {
    async fn lookup_device_verifying_key(
        &self,
        _key_id: &str,
    ) -> Result<Option<VerifyingKey>, AuthError> {
        Ok(None)
    }

    async fn lookup_device_verifying_keys_by_thumbprint(
        &self,
        kid_prefix: &[u8],
    ) -> Result<Vec<VerifyingKey>, AuthError> {
        let keys = self
            .0
            .resolve(kid_prefix, CoseAlg::Ed25519)
            .await
            .map_err(|error| AuthError::DeviceKeyLookup(error.to_string()))?;
        Ok(keys
            .iter()
            .filter_map(|key| key.ed25519_bytes())
            .filter_map(|bytes| VerifyingKey::from_bytes(&bytes).ok())
            .collect())
    }
}

/// The header-only provider the adapter wraps. It authenticates nobody, as
/// the shipped header provider does for a request with no `Authorization`.
#[derive(Clone, Default)]
pub struct HeaderlessAuth;

impl AuthProvider for HeaderlessAuth {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        Ok(CratestackContext::anonymous())
    }
}

/// The adapter a generated router takes as its `auth`, over `devices`.
pub fn cose_auth(devices: Arc<RegistryVerifierResolver>) -> CoseAuthProvider<HeaderlessAuth> {
    CoseAuthProvider::new(HeaderlessAuth, Arc::new(DeviceRegistry(devices)))
}

/// The `id` of the identity a handler saw, or `anonymous`.
pub fn principal_id(ctx: &CratestackContext) -> String {
    match ctx.auth.as_ref().and_then(|auth| auth.fields.get("id")) {
        Some(Value::String(id)) => id.clone(),
        _ => "anonymous".to_owned(),
    }
}
