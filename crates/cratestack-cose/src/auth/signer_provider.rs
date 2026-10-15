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
//!    consulted. An `Err` from it is returned as is: a header the inner
//!    provider *refuses* is never replaced by the signer. That holds only for
//!    an `Err`. A header the inner provider *accepts* but maps to anonymous
//!    (for example `SignedRequestAuthProvider` under
//!    `TransportCallerMode::Never`, for a key-only header) is not a refusal,
//!    and the signer supersedes it.
//! 2. With no verified signer in the request's extensions, the answer is the
//!    wrapped provider's, which is anonymous. A signer is only ever read from
//!    the extensions. The envelope layer inserts it there after the seal
//!    verified, and nothing on the wire can set an extension, so a header or a
//!    body cannot fake one.
//! 3. A verified signer is authenticated only if a registry still holds its
//!    key, matched by full thumbprint. The service registry is asked first,
//!    then, **only if device authentication is switched on**, the device
//!    registry. That is the order `cratestack-auth`'s verifier uses, so a
//!    device enrolment can never relabel a service key and a device registry
//!    outage does not fail a service signer. An unknown or revoked key, or a
//!    device key while the gate is off, gets the wrapped provider's anonymous
//!    answer, not an error, so policy refuses it as it refuses any anonymous
//!    caller. A backend failure of a registry that is asked is an `Err`.
//!
//! # Device authentication is opt-in
//!
//! A device key proves a device, not a user. Like `TransportCallerMode::Never`
//! in `cratestack-auth`, this adapter does not turn a key alone into a caller
//! unless told to. By default a service key authenticates (services are
//! machine principals by design) and a device key does not: it resolves to
//! anonymous, without a lookup in the device registry and never as an error.
//! [`CoseAuthProvider::authenticate_devices`] switches device keys on.
//!
//! # Security
//!
//! With device authentication **on**, wrapping a provider with
//! [`CoseAuthProvider`] makes **any request sealed by an enrolled device key
//! authenticated**, with no user (no `id_jwt`), on every method, whatever the
//! inner provider's `TransportCallerMode` says. `auth() != null` then means
//! "holds an enrolled device or service key". It no longer means "a user
//! signed in". What bounds access is revocation of the key, not token expiry:
//! a device key stays good until it is removed from the registry. Turn it on
//! only where device enrolment itself is gated, since whoever can enrol a key
//! can then pass every `auth() != null` rule. A rule that needs a signed-in
//! user cannot rely on `auth() != null` alone.
//!
//! Even with the gate off, a service key makes `auth() != null` true for the
//! holder of that key.
//!
//! The envelope's "Required" mode does not bind a header identity to the
//! signer. When the inner provider authenticates from a header, that identity
//! wins and the signer is not consulted, so a request may be sealed by one
//! device and act as the user in another's header.
//!
//! # Which keys can authenticate
//!
//! The device registry is a `DeviceKeyResolver`, which holds Ed25519 keys
//! only. An ESP256 or WebCrypto device key therefore authenticates only
//! through the service registry ([`CoseAuthProvider::with_service_keys`]),
//! which is not behind the device gate; offered to the device registry it is
//! anonymous, without a lookup.
//!
//! # The principal
//!
//! The context carries `id` (`cose:<thumbprint>`, the default
//! `ThumbprintPrincipal` form; with another prefix the two differ), `kind`
//! (`device` or `service`), `thumbprint` (bare lowercase hex) and `alg` (the
//! COSE algorithm id, an `Int`).

use std::fmt;
use std::sync::Arc;

use cratestack_auth::DeviceKeyResolver;
use cratestack_core::{
    AuthProvider, CratestackContext, CratestackError, RequestContext, VerifiedSigner,
};

use super::signer_principal::{SignerRegistries, authenticated};
use crate::keys::CoseVerifierResolver;

/// Wraps an [`AuthProvider`] and authenticates a COSE-verified signer through
/// a service key registry and, once switched on, the device key registry.
///
/// Both registries must be the same ones the envelope verifies with, or
/// stricter. A device the envelope accepts but this adapter does not know is
/// refused by policy, so revoking a key in either registry revokes it here.
/// The service registry is asked before the device registry, and the device
/// registry only after [`authenticate_devices`](Self::authenticate_devices).
///
/// ```text
/// // Services only (the default): a device key is anonymous.
/// let provider = CoseAuthProvider::new(header_provider, device_registry)
///     .with_service_keys(service_registry);
/// // Also devices, where device enrolment is gated.
/// let provider = provider.authenticate_devices(true);
/// ```
///
/// # Header refusals and the signer
///
/// A header the inner provider refuses (`Err`) is never replaced by the
/// signer. A header it accepts but maps to anonymous, such as a key-only
/// `SignedRequestAuthProvider` header under `TransportCallerMode::Never`, is
/// not a refusal: the signer supersedes it. The "Required" envelope mode does
/// not bind a header identity to the signer either; when the inner provider
/// authenticates, its identity wins and the signer is not looked at.
///
/// # Security
///
/// A device key proves a device, not a user, so device authentication is off by
/// default and a device signer is anonymous. With
/// [`authenticate_devices(true)`](Self::authenticate_devices), any request
/// sealed by an enrolled device key is authenticated, with no user (no
/// `id_jwt`), on every method, regardless of the inner provider's
/// `TransportCallerMode`. `auth() != null` then means "holds an enrolled device
/// or service key", and revoking the key, not token expiry, is what bounds
/// access. Turn it on only where device enrolment itself is gated. A service
/// key authenticates either way. See the module docs.
///
/// An ESP256 or WebCrypto device key can authenticate only through the
/// service registry (not behind the device gate), because `DeviceKeyResolver`
/// is Ed25519-only.
///
/// The `id` is `cose:<thumbprint>` only with the default `ThumbprintPrincipal`
/// prefix. The context also carries `kind`, `thumbprint` and `alg`.
#[derive(Clone)]
pub struct CoseAuthProvider<A> {
    inner: A,
    registries: SignerRegistries,
}

impl<A> CoseAuthProvider<A> {
    /// Wrap `inner`. `devices`, a [`DeviceKeyResolver`] (its thumbprint lookup
    /// is the one used), is consulted only after
    /// [`authenticate_devices(true)`](Self::authenticate_devices).
    pub fn new(inner: A, devices: Arc<dyn DeviceKeyResolver>) -> Self {
        Self {
            inner,
            registries: SignerRegistries::new(devices),
        }
    }

    /// Switch device authentication on or off (off by default).
    ///
    /// Off, a verified device signer is anonymous: the device registry is not
    /// asked, so it is never an error either, and policy refuses it. On, a
    /// device key the registry still holds authenticates as `kind` `device`
    /// (see the type docs, "Security": this makes the key alone a caller).
    /// Service keys ([`with_service_keys`](Self::with_service_keys)) are not
    /// affected.
    pub fn authenticate_devices(mut self, enabled: bool) -> Self {
        self.registries = self.registries.with_devices_enabled(enabled);
        self
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
