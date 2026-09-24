//! Building a [`CoseEnvelope`].

use std::sync::Arc;
use std::time::Duration;

use cratestack_core::{CratestackError, NonceStore};

use super::{CoseEnvelope, CoseRole, Inner};
use crate::alg::CoseMode;
use crate::keys::{CoseSigner, CoseVerifierResolver};
use crate::replay::{self, DEFAULT_SKEW_SECS};
use crate::thumbprint::KID_LEN;

/// The clock `iat` is read from and checked against: Unix seconds.
/// Injectable so the shared vectors can pin `iat`.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// Where a request's `cti` comes from. The default is 16 random bytes
/// (§5, `nonce` mode). Injectable so the shared vectors can pin `cti`, in
/// both the 16-byte and the 2-byte counter shape. It must return 1 to 4 or
/// 16 bytes, or sealing fails with a `500`.
pub type CtiSource = Arc<dyn Fn() -> Result<Vec<u8>, CratestackError> + Send + Sync>;

/// Configuration for a [`CoseEnvelope`]; see [`CoseEnvelope::server`] and
/// [`CoseEnvelope::client`].
pub struct CoseEnvelopeBuilder {
    inner: Inner,
}

impl CoseEnvelope {
    /// A server envelope: opens requests (verifying with keys from
    /// `resolver`, recording `(kid, cti)` in `nonce_store`) and seals
    /// responses with `signer`.
    pub fn server(
        mode: CoseMode,
        signer: Arc<dyn CoseSigner>,
        resolver: Arc<dyn CoseVerifierResolver>,
        nonce_store: Arc<dyn NonceStore>,
    ) -> CoseEnvelopeBuilder {
        CoseEnvelopeBuilder::new(CoseRole::Server, mode, signer, resolver, Some(nonce_store))
    }

    /// A client envelope: seals requests with `signer` and opens responses
    /// with keys from `resolver` (typically the server keys pinned at
    /// enrolment, §8).
    pub fn client(
        mode: CoseMode,
        signer: Arc<dyn CoseSigner>,
        resolver: Arc<dyn CoseVerifierResolver>,
    ) -> CoseEnvelopeBuilder {
        CoseEnvelopeBuilder::new(CoseRole::Client, mode, signer, resolver, None)
    }
}

impl CoseEnvelopeBuilder {
    fn new(
        role: CoseRole,
        mode: CoseMode,
        signer: Arc<dyn CoseSigner>,
        resolver: Arc<dyn CoseVerifierResolver>,
        nonce_store: Option<Arc<dyn NonceStore>>,
    ) -> Self {
        Self {
            inner: Inner {
                role,
                mode,
                signer,
                resolver,
                nonce_store,
                skew_secs: DEFAULT_SKEW_SECS,
                clock: Arc::new(replay::system_clock),
                cti: Arc::new(replay::random_cti),
            },
        }
    }

    /// How far `iat` may be from the opener's clock, either way. Default
    /// 300 s. Sub-second parts are ignored.
    pub fn skew(mut self, skew: Duration) -> Self {
        self.inner.skew_secs = skew.as_secs();
        self
    }

    pub fn clock(mut self, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.inner.clock = Arc::new(clock);
        self
    }

    pub fn cti_source(
        mut self,
        cti: impl Fn() -> Result<Vec<u8>, CratestackError> + Send + Sync + 'static,
    ) -> Self {
        self.inner.cti = Arc::new(cti);
        self
    }

    /// A nonce store for a client envelope that also opens requests through
    /// [`CoseEnvelope::open_request`] (a relay, or a test).
    pub fn nonce_store(mut self, store: Arc<dyn NonceStore>) -> Self {
        self.inner.nonce_store = Some(store);
        self
    }

    /// Check the configuration. Fails when the signer's algorithm belongs
    /// to the other mode, its `kid` is not 8 bytes, or the skew does not
    /// fit in an `i64` of seconds.
    pub fn build(self) -> Result<CoseEnvelope, CratestackError> {
        let inner = self.inner;
        if inner.signer.alg().mode() != inner.mode {
            return Err(CratestackError::Validation(
                "the signer's algorithm does not belong to the envelope's mode".to_owned(),
            ));
        }
        if inner.signer.kid().len() != KID_LEN {
            return Err(CratestackError::Validation(
                "a COSE kid is the 8-byte RFC 9679 thumbprint prefix".to_owned(),
            ));
        }
        if i64::try_from(inner.skew_secs).is_err() {
            return Err(CratestackError::Validation(
                "clock skew too large".to_owned(),
            ));
        }
        Ok(CoseEnvelope {
            inner: Arc::new(inner),
        })
    }
}
