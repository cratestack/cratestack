//! [`CoseAuthProvider`]: a COSE-verified signer becomes an authenticated
//! principal (cratestack#1077; ADR 0006 §12).
//!
//! A signed client sends no `Authorization` header, so the header-based
//! provider has nothing to authenticate. This adapter is the first-success
//! chain `first_success(inner).or(CoseSignerProvider)` (ADR 0021 §5,
//! cratestack#1243): the wrapped provider, then the signer the envelope layer
//! verified, which [`CoseSignerProvider`] looks up in the registries.
//!
//! The rules, in order (the chain's, applied to these two members):
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
//!    outage does not fail a service signer. Unlike that verifier, which
//!    treats a JWKS/service error as a miss, a service-registry error here is
//!    NOT a miss: it fails the request (`500`), including for device signers.
//!    This fails closed. An unknown or revoked key, or a device key while the
//!    gate is off, gets the wrapped provider's anonymous answer, so policy
//!    refuses it. A backend failure of a registry that is asked is an `Err`.
//!
//! # Device authentication is opt-in
//!
//! A device key proves a device, not a user. Like `TransportCallerMode::Never`
//! in `cratestack-auth`, this adapter does not turn a key alone into a caller
//! unless told to. By default a service key authenticates (services are
//! machine principals by design) and a device key does not: it resolves to
//! anonymous, without a lookup in the device registry. A failing service
//! registry still errors (`500`), as it is asked first.
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
//! holder of that key. So [`CoseAuthProvider::with_service_keys`] must hold
//! service keys only: a device key placed there authenticates as a service
//! whatever the gate, because the service registry is asked first.
//!
//! The envelope's "Required" mode does not bind a header identity to the
//! signer. When the inner provider authenticates from a header, that identity
//! wins and the signer is not consulted, so a request may be sealed by one
//! device and act as the user in another's header.
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
    AuthProvider, CratestackContext, CratestackError, Credential, RequestContext, first_success,
};

use super::signer_member::CoseSignerProvider;
use crate::keys::CoseVerifierResolver;

/// Wraps an [`AuthProvider`] and authenticates a COSE-verified signer through
/// a service key registry and, once switched on, the device key registry.
///
/// Each registry must be the one the envelope verifies with, or stricter, so
/// revoking a key there revokes it here. But `with_service_keys` must hold
/// service keys only (see "Security"): do not pass it a resolver that also
/// holds device keys.
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
/// `with_service_keys` must hold service keys only; a device key placed there
/// authenticates as a service whatever the gate. The service registry is asked
/// first and authenticates regardless of `authenticate_devices`, so passing it
/// the resolver devices enrol into (for example the same
/// `RegistryVerifierResolver` the envelope verifies with) makes every enrolled
/// device a `kind: service` caller with the gate off.
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
    signer: CoseSignerProvider,
}

impl<A> CoseAuthProvider<A> {
    /// Wrap `inner`. `devices`, a [`DeviceKeyResolver`] (its thumbprint lookup
    /// is the one used), is consulted only after
    /// [`authenticate_devices(true)`](Self::authenticate_devices).
    pub fn new(inner: A, devices: Arc<dyn DeviceKeyResolver>) -> Self {
        Self {
            inner,
            signer: CoseSignerProvider::new(devices),
        }
    }

    /// Switch device authentication on or off (off by default).
    ///
    /// Off, a verified device signer is anonymous: the device registry is not
    /// asked and policy refuses it (a failing service registry, asked first,
    /// still errors with `500`). On, a
    /// device key the registry still holds authenticates as `kind` `device`
    /// (see the type docs, "Security": this makes the key alone a caller).
    /// Service keys ([`with_service_keys`](Self::with_service_keys)) are not
    /// affected.
    pub fn authenticate_devices(mut self, enabled: bool) -> Self {
        self.signer = self.signer.authenticate_devices(enabled);
        self
    }

    /// Also authenticate verified service keys, which `services` holds under
    /// any algorithm it verifies (for example a `RegistryVerifierResolver`).
    /// `services` must hold service keys only: a device key here authenticates
    /// as a service whatever the gate.
    pub fn with_service_keys(mut self, services: Arc<dyn CoseVerifierResolver>) -> Self {
        self.signer = self.signer.with_service_keys(services);
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
        // The members are cheap to clone (`Arc`s and the inner provider's own
        // `Clone`); the chain is the public combinator, not a private path.
        first_success(self.inner.clone())
            .or(self.signer.clone())
            .authenticate(request)
            .await
    }

    fn credential<'r>(&self, request: &RequestContext<'r>) -> Credential<'r> {
        first_success(self.inner.clone())
            .or(self.signer.clone())
            .credential(request)
    }
}
