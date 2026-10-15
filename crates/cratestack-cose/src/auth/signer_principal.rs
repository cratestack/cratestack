//! The signer-to-principal lookup behind [`CoseAuthProvider`](super::CoseAuthProvider)
//! (cratestack#1077).
//!
//! A `VerifiedSigner` is a recorded fact, not an identity (ADR 0006 §12). This
//! module asks the registries, **now**, whether they still hold the key that
//! verified, and only then names the principal the signer authenticates as.
//! A revoked key is therefore refused on the next request even though the
//! envelope verified it a moment ago.

use std::sync::Arc;

use cratestack_auth::DeviceKeyResolver;
use cratestack_core::{CratestackContext, CratestackError, Value, VerifiedSigner};

use super::device_resolver::DeviceKeyCoseResolver;
use crate::alg::CoseAlg;
use crate::keys::CoseVerifierResolver;
use crate::thumbprint::KID_LEN;

/// The registry that still holds the key that verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SignerKind {
    Device,
    Service,
}

impl SignerKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::Service => "service",
        }
    }
}

/// The registries a verified signer is checked against. Both are reached
/// through [`CoseVerifierResolver`], the trait the envelope verifies with, so
/// a revocation in either takes effect on the next request.
#[derive(Clone)]
pub(super) struct SignerRegistries {
    devices: Arc<dyn CoseVerifierResolver>,
    services: Option<Arc<dyn CoseVerifierResolver>>,
}

impl SignerRegistries {
    pub(super) fn new(devices: Arc<dyn DeviceKeyResolver>) -> Self {
        Self {
            devices: Arc::new(DeviceKeyCoseResolver::new(devices)),
            services: None,
        }
    }

    pub(super) fn with_services(mut self, services: Arc<dyn CoseVerifierResolver>) -> Self {
        self.services = Some(services);
        self
    }

    /// The registry that still holds the key that verified `signer`, or `None`
    /// when the key is unknown or revoked. A backend `Err` stays an error: an
    /// outage must not look like an unknown key (ADR 0006 §10, a `500`).
    pub(super) async fn vouch(
        &self,
        signer: &VerifiedSigner,
    ) -> Result<Option<SignerKind>, CratestackError> {
        let thumbprint = *signer.thumbprint();
        let kid = &thumbprint[..KID_LEN];
        // The kid names its key by the thumbprint's prefix. A signer whose kid
        // disagrees with its thumbprint names no key at all.
        if signer.kid() != kid {
            return Ok(None);
        }
        let Some(alg) = CoseAlg::from_id(signer.alg()) else {
            return Ok(None);
        };
        if holds(self.devices.as_ref(), kid, alg, &thumbprint).await? {
            return Ok(Some(SignerKind::Device));
        }
        if let Some(services) = self.services.as_deref()
            && holds(services, kid, alg, &thumbprint).await?
        {
            return Ok(Some(SignerKind::Service));
        }
        Ok(None)
    }
}

/// Whether `registry` holds a key for `kid` and `alg` whose full thumbprint is
/// `thumbprint`. Matching on the thumbprint, not the 8-byte kid, is what keeps
/// two keys that share a kid apart.
async fn holds(
    registry: &dyn CoseVerifierResolver,
    kid: &[u8],
    alg: CoseAlg,
    thumbprint: &[u8; 32],
) -> Result<bool, CratestackError> {
    Ok(registry
        .resolve(kid, alg)
        .await?
        .iter()
        .any(|key| key.thumbprint() == *thumbprint))
}

/// The context a vouched signer authenticates as. `id` is `cose:<thumbprint>`,
/// the string `ThumbprintPrincipal` charges the same signer to, so rate-limit
/// buckets and policy rules name one principal.
pub(super) fn authenticated(signer: &VerifiedSigner, kind: SignerKind) -> CratestackContext {
    let thumbprint: String = signer
        .thumbprint()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    CratestackContext::authenticated([
        ("id".to_owned(), Value::String(format!("cose:{thumbprint}"))),
        ("kind".to_owned(), Value::String(kind.label().to_owned())),
        ("thumbprint".to_owned(), Value::String(thumbprint)),
        ("alg".to_owned(), Value::Int(signer.alg())),
    ])
}
