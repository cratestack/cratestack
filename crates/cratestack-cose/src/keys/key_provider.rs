//! Mac0 keys from core's [`KeyProvider`] (ADR 0006 §1: "Adapters:
//! `KeyProvider` → Mac0 keys").
//!
//! A `KeyProvider` resolves a **string** key id to raw bytes, which is how
//! `HmacEnvelope` deployments already configure their secrets. A Mac0
//! message names its key by a thumbprint prefix instead (P0 scoping
//! decision), and a thumbprint cannot be looked up in a string-keyed store.
//! So the deployer lists their key ids, the secrets are resolved once, at
//! construction, and the thumbprints are precomputed. Needs only
//! `cratestack-core`, so it is not part of the `auth` feature.

use cratestack_core::{CratestackError, KeyProvider};

use super::hmac::{HmacSecret, HmacSigner, is_mac};
use super::traits::CoseVerifierResolver;
use super::verify_key::CoseVerifyKey;
use crate::alg::CoseAlg;

/// A fixed set of HMAC keys, loaded from a [`KeyProvider`] by string id.
///
/// Every key carries the one algorithm the set was loaded for. Rotation is
/// a new set with the new key id listed next to the old one.
#[derive(Debug, Clone)]
pub struct KeyProviderMacKeys {
    alg: CoseAlg,
    entries: Vec<Entry>,
}

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    secret: HmacSecret,
    key: CoseVerifyKey,
}

impl KeyProviderMacKeys {
    /// Resolve every id in `key_ids` through `provider`, for `alg`
    /// ([`CoseAlg::Hmac256_64`] or [`CoseAlg::Hmac256_256`]).
    ///
    /// A configuration step, so it fails loudly: with
    /// `CratestackError::Validation` for a non-MAC `alg`, an empty or
    /// repeated id list, a secret shorter than 32 bytes, or two ids that
    /// resolve to the same secret (one key under two names is a
    /// misconfiguration waiting to happen, and the two would share a `kid`);
    /// with the provider's own error when it cannot resolve an id.
    pub async fn load<P, I, S>(
        provider: &P,
        alg: CoseAlg,
        key_ids: I,
    ) -> Result<Self, CratestackError>
    where
        P: KeyProvider + ?Sized,
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if !is_mac(alg) {
            return Err(invalid("KeyProviderMacKeys needs an HMAC algorithm"));
        }
        let mut entries: Vec<Entry> = Vec::new();
        for name in key_ids {
            let name = name.into();
            if entries.iter().any(|entry| entry.name == name) {
                return Err(invalid(format!("key id {name:?} is listed twice")));
            }
            let secret =
                HmacSecret::new(provider.resolve_signing_key(&name).await?).map_err(|_| {
                    invalid(format!("the secret for key id {name:?} is under 32 bytes"))
                })?;
            let key = CoseVerifyKey::from_hmac_secret(alg, secret.clone());
            if entries
                .iter()
                .any(|entry| entry.key.thumbprint() == key.thumbprint())
            {
                return Err(invalid(format!(
                    "key id {name:?} resolves to the same secret as another id"
                )));
            }
            entries.push(Entry { name, secret, key });
        }
        if entries.is_empty() {
            return Err(invalid("KeyProviderMacKeys needs at least one key id"));
        }
        Ok(Self { alg, entries })
    }

    /// The algorithm every key in the set verifies.
    pub fn alg(&self) -> CoseAlg {
        self.alg
    }

    /// The configured key ids with the `kid` each one travels as, for logs
    /// and for mapping a verified message back to its configured name.
    pub fn kids(&self) -> impl Iterator<Item = (&str, [u8; 8])> {
        self.entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.key.kid()))
    }

    /// A signer for the configured key id `key_id`, or `None` if it was not
    /// loaded.
    pub fn signer(&self, key_id: &str) -> Option<HmacSigner> {
        let entry = self.entries.iter().find(|entry| entry.name == key_id)?;
        HmacSigner::from_secret(self.alg, entry.secret.clone()).ok()
    }
}

fn invalid(message: impl Into<String>) -> CratestackError {
    CratestackError::Validation(message.into())
}

/// Every loaded key whose `kid` and algorithm match; an unknown `kid` is
/// `Ok(vec![])`, as the resolver contract requires. Never `Err`: the
/// secrets were resolved at construction, so there is no backend left to
/// fail.
#[async_trait::async_trait]
impl CoseVerifierResolver for KeyProviderMacKeys {
    async fn resolve(
        &self,
        kid: &[u8],
        alg: CoseAlg,
    ) -> Result<Vec<CoseVerifyKey>, CratestackError> {
        Ok(self
            .entries
            .iter()
            .filter(|entry| entry.key.kid().as_slice() == kid && entry.key.supports(alg))
            .map(|entry| entry.key.clone())
            .collect())
    }
}
