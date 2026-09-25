//! An in-memory [`CoseVerifierResolver`].

use cratestack_core::CratestackError;

use super::traits::CoseVerifierResolver;
use super::verify_key::CoseVerifyKey;
use crate::alg::CoseAlg;

/// A fixed set of verification keys, indexed by their `kid`. For tests and
/// for in-process use (a service that knows its peers' keys at start-up).
/// A device-key database or a published COSE_KeySet implements
/// [`CoseVerifierResolver`] itself.
///
/// The `kid` is computed from each key, so a key can never be filed under a
/// `kid` it does not have.
#[derive(Debug, Clone, Default)]
pub struct StaticVerifierResolver {
    keys: Vec<CoseVerifyKey>,
}

impl StaticVerifierResolver {
    /// An empty resolver: every `kid` is unknown until keys are added.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `key`. Keys that share a `kid` (an 8-byte prefix collision, or
    /// one HMAC secret listed for both HMAC algorithms) are all returned
    /// for it, in the order they were added.
    pub fn with_key(mut self, key: CoseVerifyKey) -> Self {
        self.keys.push(key);
        self
    }
}

#[async_trait::async_trait]
impl CoseVerifierResolver for StaticVerifierResolver {
    async fn resolve(
        &self,
        kid: &[u8],
        alg: CoseAlg,
    ) -> Result<Vec<CoseVerifyKey>, CratestackError> {
        Ok(self
            .keys
            .iter()
            .filter(|key| key.kid().as_slice() == kid && key.supports(alg))
            .cloned()
            .collect())
    }
}
