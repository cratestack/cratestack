//! A thread-safe [`CoseVerifierResolver`] whose keys change at run time.

use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

use cratestack_core::CratestackError;

use super::traits::CoseVerifierResolver;
use super::verify_key::CoseVerifyKey;
use crate::alg::CoseAlg;
use crate::thumbprint::KID_LEN;

#[derive(Debug, Default)]
struct Keys {
    by_kid: HashMap<[u8; KID_LEN], Vec<CoseVerifyKey>>,
    len: usize,
}

/// A set of verification keys that can be registered and revoked while the
/// server runs: the resolver for devices or peers that enrol after start-up
/// (the immutable [`StaticVerifierResolver`](super::StaticVerifierResolver)
/// is fixed at construction). Holds every key type the verifier accepts:
/// Ed25519, ESP256 and the two HMAC algorithms.
///
/// Share it as an `Arc`: hand a clone to the envelope (an `Arc<dyn
/// CoseVerifierResolver>`) and keep one for [`register`](Self::register) and
/// [`revoke`](Self::revoke).
///
/// ```
/// use std::sync::Arc;
/// use cratestack_cose::{
///     CoseAlg, CoseVerifierResolver, Ed25519Signer, RegistryVerifierResolver,
/// };
///
/// # let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
/// # rt.block_on(async {
/// let registry = Arc::new(RegistryVerifierResolver::new());
/// let for_envelope: Arc<dyn CoseVerifierResolver> = registry.clone();
///
/// let key = Ed25519Signer::from_seed(&[7; 32]).verify_key();
/// assert!(for_envelope.resolve(&key.kid(), CoseAlg::Ed25519).await?.is_empty());
///
/// let kid = registry.register(key.clone())?; // a device enrols
/// assert_eq!(kid, key.kid());
/// assert_eq!(for_envelope.resolve(&kid, CoseAlg::Ed25519).await?, vec![key]);
///
/// assert_eq!(registry.revoke(&kid), 1); // and is cut off
/// assert!(for_envelope.resolve(&kid, CoseAlg::Ed25519).await?.is_empty());
/// # Ok::<(), cratestack_core::CratestackError>(())
/// # }).unwrap();
/// ```
///
/// **The `kid`** is the first 8 bytes of the key's RFC 9679 thumbprint,
/// computed from the key (so it cannot be filed under another). Keys with
/// the same `kid` (an 8-byte collision, or one HMAC secret registered for
/// both HMAC algorithms) are all kept, and all returned for it.
///
/// **Concurrency.** Every call takes one short lock and none is held across
/// an `.await`. A `register` or `revoke` is atomic: a concurrent
/// [`resolve`](CoseVerifierResolver::resolve) sees the registry wholly
/// before it or wholly after. A request whose `resolve` already returned the
/// key still finishes verifying with it, so a revocation takes effect for
/// requests that resolve after it returns, not for ones already in flight.
/// A `register` that returned is visible to every later `resolve`.
///
/// Unknown `kid`s resolve to `Ok(vec![])`, never `Err`, as the trait
/// requires. State is per process: replicas each need the same
/// registrations.
#[derive(Debug, Default)]
pub struct RegistryVerifierResolver {
    keys: RwLock<Keys>,
    max_keys: Option<usize>,
}

impl RegistryVerifierResolver {
    /// An empty registry with no bound on its size.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty registry holding at most `max_keys` keys. A server that
    /// registers keys on behalf of callers uses this to bound memory.
    ///
    /// ```
    /// use cratestack_cose::{Ed25519Signer, RegistryVerifierResolver};
    ///
    /// let registry = RegistryVerifierResolver::with_max_keys(1);
    /// registry.register(Ed25519Signer::from_seed(&[1; 32]).verify_key())?;
    /// assert!(registry
    ///     .register(Ed25519Signer::from_seed(&[2; 32]).verify_key())
    ///     .is_err());
    /// # Ok::<(), cratestack_core::CratestackError>(())
    /// ```
    pub fn with_max_keys(max_keys: usize) -> Self {
        Self {
            max_keys: Some(max_keys),
            ..Self::default()
        }
    }

    /// Register `key` and return its `kid`. Registering a key that is
    /// already present changes nothing and succeeds, even when the registry
    /// is full. Fails with `CratestackError::Conflict` when a new key would
    /// exceed the bound; the registry is then unchanged.
    pub fn register(&self, key: CoseVerifyKey) -> Result<[u8; KID_LEN], CratestackError> {
        let kid = key.kid();
        let mut keys = self.keys.write().unwrap_or_else(PoisonError::into_inner);
        if keys
            .by_kid
            .get(&kid)
            .is_some_and(|held| held.contains(&key))
        {
            return Ok(kid);
        }
        if self.max_keys.is_some_and(|max| keys.len >= max) {
            return Err(CratestackError::Conflict(
                "the verification key registry is full".to_owned(),
            ));
        }
        keys.by_kid.entry(kid).or_default().push(key);
        keys.len += 1;
        Ok(kid)
    }

    /// Remove every key filed under `kid` and return how many there were
    /// (`0` for an unknown or malformed `kid`: revoking twice is harmless).
    /// One HMAC secret registered for both algorithms goes with it, and so
    /// does any other key whose 8-byte `kid` collides (an unrelated
    /// device). To revoke one device, use [`revoke_key`](Self::revoke_key).
    pub fn revoke(&self, kid: &[u8]) -> usize {
        let Ok(kid) = <[u8; KID_LEN]>::try_from(kid) else {
            return 0;
        };
        let mut keys = self.keys.write().unwrap_or_else(PoisonError::into_inner);
        let removed = keys.by_kid.remove(&kid).map_or(0, |held| held.len());
        keys.len -= removed;
        removed
    }

    /// Remove exactly `key` and nothing else that shares its `kid`. `true`
    /// if it was registered.
    pub fn revoke_key(&self, key: &CoseVerifyKey) -> bool {
        let kid = key.kid();
        let mut keys = self.keys.write().unwrap_or_else(PoisonError::into_inner);
        let Some(held) = keys.by_kid.get_mut(&kid) else {
            return false;
        };
        let Some(at) = held.iter().position(|candidate| candidate == key) else {
            return false;
        };
        held.remove(at);
        if held.is_empty() {
            keys.by_kid.remove(&kid);
        }
        keys.len -= 1;
        true
    }

    /// The number of registered keys.
    pub fn len(&self) -> usize {
        self.keys.read().unwrap_or_else(PoisonError::into_inner).len
    }

    /// Whether no key is registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl CoseVerifierResolver for RegistryVerifierResolver {
    async fn resolve(
        &self,
        kid: &[u8],
        alg: CoseAlg,
    ) -> Result<Vec<CoseVerifyKey>, CratestackError> {
        let Ok(kid) = <[u8; KID_LEN]>::try_from(kid) else {
            return Ok(Vec::new());
        };
        let keys = self.keys.read().unwrap_or_else(PoisonError::into_inner);
        Ok(keys
            .by_kid
            .get(&kid)
            .into_iter()
            .flatten()
            .filter(|key| key.supports(alg))
            .cloned()
            .collect())
    }
}
