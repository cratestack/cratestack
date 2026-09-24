//! An in-memory [`CoseVerifierResolver`].

use cratestack_core::CratestackError;

use super::traits::CoseVerifierResolver;
use super::verify_key::CoseVerifyKey;
use crate::alg::CoseAlg;
use crate::thumbprint::KID_LEN;

/// A fixed set of verification keys, indexed by their `kid`. For tests and
/// for in-process use (a service that knows its peers' keys at start-up).
/// A device-key database or a published COSE_KeySet implements
/// [`CoseVerifierResolver`] itself.
///
/// The `kid` is computed from each key when it is added, so a key can never
/// be filed under a `kid` it does not have.
#[derive(Debug, Clone, Default)]
pub struct StaticVerifierResolver {
    keys: Vec<([u8; KID_LEN], CoseVerifyKey)>,
}

impl StaticVerifierResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_key(mut self, key: CoseVerifyKey) -> Self {
        self.keys.push((key.kid(), key));
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
            .filter(|(key_kid, key)| key_kid.as_slice() == kid && key.supports(alg))
            .map(|(_, key)| key.clone())
            .collect())
    }
}
