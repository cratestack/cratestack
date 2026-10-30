//! [`CoseSignerProvider`]: the COSE-verified signer as an [`AuthProvider`]
//! of its own, the member of [`CoseAuthProvider`](super::CoseAuthProvider)'s
//! first-success chain that reads the signer (cratestack#1243, ADR 0021 §5).
//!
//! Its credential is the [`VerifiedSigner`] the envelope layer recorded in
//! the request's extensions: present when there is one, absent otherwise.
//! A signer whose key a registry still holds authenticates (see
//! `CoseAuthProvider`'s rule 3); an unknown or revoked key, or a device key
//! while device authentication is off, is anonymous; a failing registry that
//! is asked is an `Err` (`500`).

use std::sync::Arc;

use cratestack_auth::DeviceKeyResolver;
use cratestack_core::{
    AuthProvider, CratestackContext, CratestackError, Credential, RequestContext, VerifiedSigner,
};

use super::signer_principal::{SignerRegistries, authenticated};
use crate::keys::CoseVerifierResolver;

/// The COSE-verified signer as an [`AuthProvider`]. Compose it with another
/// provider through `cratestack::first_success(...).or(...)`, or use
/// [`CoseAuthProvider`](super::CoseAuthProvider), which does exactly that.
/// The "Security" sections of `CoseAuthProvider` apply unchanged.
#[derive(Clone)]
pub struct CoseSignerProvider {
    registries: SignerRegistries,
}

impl CoseSignerProvider {
    /// `devices` (its thumbprint lookup) is asked only after
    /// [`authenticate_devices(true)`](Self::authenticate_devices).
    pub fn new(devices: Arc<dyn DeviceKeyResolver>) -> Self {
        Self {
            registries: SignerRegistries::new(devices),
        }
    }

    /// See `CoseAuthProvider::authenticate_devices`.
    pub fn authenticate_devices(mut self, enabled: bool) -> Self {
        self.registries = self.registries.with_devices_enabled(enabled);
        self
    }

    /// See `CoseAuthProvider::with_service_keys`: service keys only.
    pub fn with_service_keys(mut self, services: Arc<dyn CoseVerifierResolver>) -> Self {
        self.registries = self.registries.with_services(services);
        self
    }
}

impl std::fmt::Debug for CoseSignerProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoseSignerProvider").finish_non_exhaustive()
    }
}

impl AuthProvider for CoseSignerProvider {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        let Some(signer) = request.extensions.get::<VerifiedSigner>() else {
            return Ok(CratestackContext::anonymous());
        };
        Ok(match self.registries.vouch(signer).await? {
            Some(kind) => authenticated(signer, kind),
            None => CratestackContext::anonymous(),
        })
    }

    /// Present when the envelope layer recorded a verified signer. Only the
    /// extensions are read: nothing on the wire can set one.
    fn credential<'r>(&self, request: &RequestContext<'r>) -> Credential<'r> {
        if request.extensions.get::<VerifiedSigner>().is_some() {
            Credential::Present { redact: None }
        } else {
            Credential::Absent
        }
    }
}
