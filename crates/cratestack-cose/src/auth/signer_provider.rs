//! [`CoseAuthProvider`]: a COSE-verified signer becomes an authenticated
//! principal (cratestack#1077; ADR 0006 §12).
//!
//! A signed client sends no `Authorization` header, so the header-based
//! provider has nothing to authenticate. This adapter wraps that provider and
//! falls back to the signer the envelope layer verified, which it looks up in
//! the registries.
//!
//! The rules, in order:
//!
//! 1. The wrapped provider runs first. An authenticated answer (a valid
//!    `Authorization` header) is returned as is, and the registries are not
//!    consulted. An `Err` from it is returned as is: a bad header is refused,
//!    never replaced by the signer.
//! 2. With no verified signer in the request's extensions, the answer is the
//!    wrapped provider's, which is anonymous. A signer is only ever read from
//!    the extensions. The envelope layer inserts it there after the seal
//!    verified, and nothing on the wire can set an extension, so a header or a
//!    body cannot fake one.
//! 3. A verified signer is authenticated only if a registry still holds its
//!    key, matched by full thumbprint. An unknown or revoked key gets the
//!    wrapped provider's anonymous answer, not an error, so policy refuses it
//!    as it refuses any anonymous caller. A backend failure is an `Err`.

use std::fmt;
use std::sync::Arc;

use cratestack_auth::DeviceKeyResolver;
use cratestack_core::{
    AuthProvider, CratestackContext, CratestackError, RequestContext, VerifiedSigner,
};

use super::signer_principal::{SignerRegistries, authenticated};
use crate::keys::CoseVerifierResolver;

/// Wraps an [`AuthProvider`] and authenticates a COSE-verified signer through
/// the device key registry and, optionally, a service key registry.
///
/// ```ignore
/// let provider = CoseAuthProvider::new(header_provider, device_registry)
///     .with_service_keys(service_registry);
/// ```
///
/// Both registries must be the same ones the envelope verifies with, or
/// stricter. A device the envelope accepts but this adapter does not know is
/// refused by policy, so revoking a key in either registry revokes it here.
#[derive(Clone)]
pub struct CoseAuthProvider<A> {
    inner: A,
    registries: SignerRegistries,
}

impl<A> CoseAuthProvider<A> {
    /// Authenticate verified devices through `devices`, a
    /// [`DeviceKeyResolver`] (its thumbprint lookup is the one used).
    pub fn new(inner: A, devices: Arc<dyn DeviceKeyResolver>) -> Self {
        Self {
            inner,
            registries: SignerRegistries::new(devices),
        }
    }

    /// Also authenticate verified service keys, which `services` holds under
    /// any algorithm it verifies (for example a `RegistryVerifierResolver`).
    pub fn with_service_keys(mut self, services: Arc<dyn CoseVerifierResolver>) -> Self {
        self.registries = self.registries.with_services(services);
        self
    }
}

impl<A: fmt::Debug> fmt::Debug for CoseAuthProvider<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoseAuthProvider")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl<A: AuthProvider> AuthProvider for CoseAuthProvider<A> {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        let from_inner = self.inner.authenticate(request).await.map_err(Into::into)?;
        if from_inner.is_authenticated() {
            return Ok(from_inner);
        }
        let Some(signer) = request.extensions.get::<VerifiedSigner>() else {
            return Ok(from_inner);
        };
        match self.registries.vouch(signer).await? {
            Some(kind) => Ok(authenticated(signer, kind)),
            None => Ok(from_inner),
        }
    }
}
