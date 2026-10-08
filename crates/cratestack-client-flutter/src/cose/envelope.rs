//! [`FlutterClientEnvelope`]: a client's COSE envelope behind the bridge.

use std::sync::Arc;

use bytes::Bytes;
use cratestack_core::CratestackError;
use cratestack_cose::{
    CoseEnvelope, CoseSigner, Ed25519Signer, HmacSigner, StaticVerifierResolver,
};

use zeroize::{Zeroize, Zeroizing};

use super::error::FlutterCoseError;
use super::types::{
    FlutterCallBinding, FlutterCoseAlg, FlutterCoseMode, FlutterOpened, FlutterSealOptions,
    FlutterServerKey,
};

/// Seals requests and opens responses for one service.
///
/// Opaque across the bridge: the key never leaves Rust. The signers of
/// this release hold their key in memory (an HMAC secret, an Ed25519
/// seed); a key in the Android Keystore or the Secure Enclave comes with
/// the callback signer of a later release. All crypto, the canonical query
/// and the AAD are `cratestack-cose`'s: this type only maps types.
#[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(opaque))]
pub struct FlutterClientEnvelope {
    inner: CoseEnvelope,
    audience: String,
    kid: Vec<u8>,
}

impl FlutterClientEnvelope {
    /// A COSE_Mac0 envelope over a shared secret (at least 32 bytes).
    /// `alg` must be one of the HMAC algorithms.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync))]
    pub fn hmac(
        alg: FlutterCoseAlg,
        secret: Vec<u8>,
        server_keys: Vec<FlutterServerKey>,
        audience: String,
        options: Option<FlutterSealOptions>,
    ) -> Result<Self, FlutterCoseError> {
        let signer = HmacSigner::new(alg.into(), secret)?;
        Self::from_signer(Arc::new(signer), server_keys, audience, options)
    }

    /// A COSE_Sign1 envelope signing with the Ed25519 key of `seed`, held
    /// in memory: for tests and service credentials, never a device key.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync))]
    pub fn ed25519_seed(
        seed: Vec<u8>,
        server_keys: Vec<FlutterServerKey>,
        audience: String,
        options: Option<FlutterSealOptions>,
    ) -> Result<Self, FlutterCoseError> {
        // Both copies of the seed are wiped on drop.
        let seed = Zeroizing::new(seed);
        let key: Zeroizing<[u8; 32]> =
            Zeroizing::new(seed.as_slice().try_into().map_err(|_| {
                CratestackError::Validation("an Ed25519 seed is 32 bytes".to_owned())
            })?);
        Self::from_signer(
            Arc::new(Ed25519Signer::from_seed(&key)),
            server_keys,
            audience,
            options,
        )
    }

    /// An envelope around any signer. Not bridged: it is how the tests
    /// drive ESP256 and how a keystore signer will plug in.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(ignore))]
    pub fn from_signer(
        signer: Arc<dyn CoseSigner>,
        mut server_keys: Vec<FlutterServerKey>,
        audience: String,
        options: Option<FlutterSealOptions>,
    ) -> Result<Self, FlutterCoseError> {
        if audience.is_empty() {
            return Err(CratestackError::Validation("the audience is empty".to_owned()).into());
        }
        let mut resolver = StaticVerifierResolver::new();
        for key in &server_keys {
            resolver = resolver.with_key(key.to_verify_key()?);
        }
        // A Mac0 server key is the shared secret: the resolver holds its own
        // (wiped) copy now.
        for key in &mut server_keys {
            key.bytes.zeroize();
        }
        let kid = signer.kid().to_vec();
        let mut builder = CoseEnvelope::client(signer.alg().mode(), signer, Arc::new(resolver));
        let options = options.unwrap_or_default();
        if let Some(iat) = options.fixed_iat {
            builder = builder.clock(move || iat);
        }
        if let Some(cti) = options.fixed_cti {
            builder = builder.cti_source(move || Ok(cti.clone()));
        }
        Ok(Self {
            inner: builder.build()?,
            audience,
            kid,
        })
    }

    /// COSE_Sign1 or COSE_Mac0.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync, getter))]
    pub fn mode(&self) -> FlutterCoseMode {
        self.inner.mode().into()
    }

    /// The `Content-Type` (and `Accept`) of sealed bodies.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync, getter))]
    pub fn media_type(&self) -> String {
        self.inner.mode().media_type().to_owned()
    }

    /// The signer's 8-byte `kid`.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(sync, getter))]
    pub fn kid(&self) -> Vec<u8> {
        self.kid.clone()
    }

    /// Seal `payload` (already CBOR) as the request described by `binding`.
    pub async fn seal_request(
        &self,
        payload: Vec<u8>,
        binding: FlutterCallBinding,
    ) -> Result<Vec<u8>, FlutterCoseError> {
        let call = binding.with_audience(&self.audience)?;
        let sealed = self.inner.seal_request(&payload, &call.request()).await?;
        Ok(sealed.to_vec())
    }

    /// Open the response `body` to `sealed_request` (the exact bytes that
    /// were sent), which came back with HTTP `status`.
    pub async fn open_response(
        &self,
        body: Vec<u8>,
        binding: FlutterCallBinding,
        sealed_request: Vec<u8>,
        status: u16,
    ) -> Result<FlutterOpened, FlutterCoseError> {
        let call = binding.with_audience(&self.audience)?;
        let opened = self
            .inner
            .open_response(Bytes::from(body), &call.response(&sealed_request, status))
            .await?;
        opened.try_into()
    }
}
