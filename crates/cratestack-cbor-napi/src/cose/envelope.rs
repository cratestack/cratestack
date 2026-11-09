//! The pure half of the COSE binding: everything but the napi plumbing. The
//! class in `shell.rs` is a thin shell around [`Envelope`].

use std::sync::Arc;

use cratestack_cose::{
    CoseEnvelope, CoseMode, CoseSigner, Ed25519Signer, HmacSigner, P256Signer,
    StaticVerifierResolver,
};
use zeroize::Zeroizing;

use super::external;
use super::fail::Fail;
use super::spec::{self, SealOptions, ServerKey};

/// Seals requests and opens responses for one service (ADR 0006). All
/// crypto, the canonical query and the AAD are `cratestack-cose`'s.
pub(crate) struct Envelope {
    pub(super) inner: CoseEnvelope,
    pub(super) audience: String,
    pub(super) kid: Vec<u8>,
    pub(super) external: bool,
}

/// What a verified response resolves to.
#[derive(Debug)]
pub(crate) struct OpenedResponse {
    pub(crate) payload: Vec<u8>,
    pub(crate) payload_type: String,
    pub(crate) kid: Vec<u8>,
    pub(crate) alg: &'static str,
    pub(crate) thumbprint: Vec<u8>,
}

impl Envelope {
    /// A COSE_Mac0 envelope over a shared secret of at least 32 bytes.
    pub(crate) fn hmac(
        alg: &str,
        secret: &[u8],
        keys: &[ServerKey],
        audience: &str,
        options: SealOptions,
    ) -> Result<Envelope, Fail> {
        let signer = HmacSigner::new(spec::alg(alg)?, secret.to_vec())?;
        Self::build(Arc::new(signer), false, keys, audience, options)
    }

    /// A COSE_Sign1 envelope signing with the Ed25519 key of a 32-byte seed.
    pub(crate) fn ed25519_seed(
        seed: &[u8],
        keys: &[ServerKey],
        audience: &str,
        options: SealOptions,
    ) -> Result<Envelope, Fail> {
        // The copy is wiped on drop.
        let seed: Zeroizing<[u8; 32]> = Zeroizing::new(
            seed.try_into()
                .map_err(|_| Fail::misuse("an Ed25519 seed is 32 bytes"))?,
        );
        Self::build(
            Arc::new(Ed25519Signer::from_seed(&seed)),
            false,
            keys,
            audience,
            options,
        )
    }

    /// A COSE_Sign1 envelope signing with the P-256 key of a 32-byte scalar.
    pub(crate) fn p256_scalar(
        scalar: &[u8],
        keys: &[ServerKey],
        audience: &str,
        options: SealOptions,
    ) -> Result<Envelope, Fail> {
        let scalar: Zeroizing<[u8; 32]> = Zeroizing::new(
            scalar
                .try_into()
                .map_err(|_| Fail::misuse("a P-256 scalar is 32 bytes"))?,
        );
        Self::build(
            Arc::new(P256Signer::from_scalar(&scalar)?),
            false,
            keys,
            audience,
            options,
        )
    }

    /// A COSE_Sign1 envelope whose signature comes from outside, in two
    /// steps (see `external.rs`).
    pub(crate) fn external(
        alg: &str,
        public_key: &[u8],
        keys: &[ServerKey],
        audience: &str,
        options: SealOptions,
    ) -> Result<Envelope, Fail> {
        let signer = external::signer(spec::alg(alg)?, public_key)?;
        Self::build(Arc::new(signer), true, keys, audience, options)
    }

    fn build(
        signer: Arc<dyn CoseSigner>,
        external: bool,
        keys: &[ServerKey],
        audience: &str,
        options: SealOptions,
    ) -> Result<Envelope, Fail> {
        if audience.is_empty() {
            return Err(Fail::misuse("the audience is empty"));
        }
        let mut resolver = StaticVerifierResolver::new();
        for key in spec::server_keys(keys)? {
            resolver = resolver.with_key(key);
        }
        let kid = signer.kid().to_vec();
        let mut builder = CoseEnvelope::client(signer.alg().mode(), signer, Arc::new(resolver));
        if let Some(iat) = options.fixed_iat {
            builder = builder.clock(move || iat);
        }
        if let Some(cti) = options.fixed_cti {
            builder = builder.cti_source(move || Ok(cti.clone()));
        }
        Ok(Envelope {
            inner: builder.build()?,
            audience: audience.to_owned(),
            kid,
            external,
        })
    }

    /// `"sign1"` or `"mac0"`.
    pub(crate) fn mode(&self) -> &'static str {
        match self.inner.mode() {
            CoseMode::Sign1 => "sign1",
            CoseMode::Mac0 => "mac0",
        }
    }

    pub(crate) fn media_type(&self) -> &'static str {
        self.inner.mode().media_type()
    }

    pub(crate) fn kid(&self) -> &[u8] {
        &self.kid
    }
}

impl std::fmt::Debug for Envelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Envelope")
            .field("audience", &self.audience)
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}
