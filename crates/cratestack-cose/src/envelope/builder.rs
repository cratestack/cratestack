//! Building a [`CoseEnvelope`].

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use cratestack_core::{CratestackError, NonceStore};

use super::{CoseEnvelope, CoseRole, Inner};
use crate::alg::CoseMode;
use crate::keys::{CoseSigner, CoseVerifierResolver};
use crate::replay::{self, DEFAULT_SKEW_SECS};
use crate::thumbprint::KID_LEN;
use cratestack_core::{REQUEST_NONCE_LEN, RequestNonce};

/// The clock `iat` is read from and checked against: Unix seconds.
pub(crate) type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// Where a request's `cti` comes from.
pub(crate) type CtiSource = Arc<dyn Fn() -> Result<Vec<u8>, CratestackError> + Send + Sync>;

/// Where [`CoseEnvelope::request_nonce`] draws its bytes from.
pub(crate) type NonceSource =
    Arc<dyn Fn() -> Result<[u8; REQUEST_NONCE_LEN], CratestackError> + Send + Sync>;

/// How far past the build-time clock reading the skew is checked (1000
/// years), so an envelope that builds never starts failing while it runs.
const SKEW_CHECK_HORIZON_SECS: u64 = 1000 * 366 * 24 * 60 * 60;

/// Configuration for a [`CoseEnvelope`]; see [`CoseEnvelope::server`] and
/// [`CoseEnvelope::client`]. Nothing is checked until [`build`](Self::build).
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
                nonce: Arc::new(|| RequestNonce::random().map(|nonce| *nonce.as_bytes())),
            },
        }
    }

    /// How far `iat` may be from the opener's clock, either way. Default
    /// 300 s. Sub-second parts are ignored.
    ///
    /// It also sets how long a `(kid, cti)` stays in the nonce store:
    /// `2·skew + 1` seconds past `iat` (the second `skew` covers clock
    /// disagreement between replicas and the store; see `replay.rs`), so
    /// the store's working set is every request of the last `2·skew + 1`
    /// seconds. [`build`](Self::build) refuses a skew so large that this
    /// expiry would leave the representable time range.
    pub fn skew(mut self, skew: Duration) -> Self {
        self.inner.skew_secs = skew.as_secs();
        self
    }

    /// The clock `iat` is read from when sealing and checked against when
    /// opening, in Unix seconds. Default: the system clock through
    /// `chrono` (which, unlike `SystemTime`, works on
    /// `wasm32-unknown-unknown`). Injectable so tests and the shared vectors
    /// can pin `iat`. A reading before 1970 makes sealing a request fail
    /// with a `500`.
    pub fn clock(mut self, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.inner.clock = Arc::new(clock);
        self
    }

    /// Where a request's `cti` comes from. Default: 16 bytes from the
    /// operating system's CSPRNG (§5, `nonce` mode). Injectable so the
    /// shared vectors can pin `cti`, in both the 16-byte and the 2-byte
    /// counter shape. It must return 1 to 4 or 16 bytes; anything else, or
    /// an `Err`, makes sealing fail with a `500`.
    pub fn cti_source(
        mut self,
        cti: impl Fn() -> Result<Vec<u8>, CratestackError> + Send + Sync + 'static,
    ) -> Self {
        self.inner.cti = Arc::new(cti);
        self
    }

    /// Where [`CoseEnvelope::request_nonce`] gets its 16 bytes. Default:
    /// the operating system's CSPRNG. Injectable for tests and vectors.
    pub fn nonce_source(
        mut self,
        nonce: impl Fn() -> Result<[u8; REQUEST_NONCE_LEN], CratestackError> + Send + Sync + 'static,
    ) -> Self {
        self.inner.nonce = Arc::new(nonce);
        self
    }

    /// A nonce store for a client envelope that also opens requests through
    /// [`CoseEnvelope::open_request`] (a relay, or a test).
    pub fn nonce_store(mut self, store: Arc<dyn NonceStore>) -> Self {
        self.inner.nonce_store = Some(store);
        self
    }

    /// Check the configuration. Fails with `CratestackError::Validation`
    /// when the signer's algorithm belongs to the other mode, its `kid` is
    /// not 8 bytes, or the skew is so large that a nonce's expiry
    /// (`iat + 2·skew + 1`, for any `iat` the opener could accept within the
    /// next thousand years) would not be a representable time. Without that
    /// last check such an envelope would build, and then answer every valid
    /// request with a `401`.
    pub fn build(self) -> Result<CoseEnvelope, CratestackError> {
        let inner = self.inner;
        if inner.signer.alg().mode() != inner.mode {
            return Err(invalid(
                "the signer's algorithm does not belong to the envelope's mode",
            ));
        }
        if inner.signer.kid().len() != KID_LEN {
            return Err(invalid(
                "a COSE kid is the 8-byte RFC 9679 thumbprint prefix",
            ));
        }
        let now = u64::try_from((inner.clock)()).unwrap_or(0);
        let latest_iat = now
            .checked_add(SKEW_CHECK_HORIZON_SECS)
            .and_then(|at| at.checked_add(inner.skew_secs));
        if latest_iat
            .and_then(|iat| replay::nonce_expiry(iat, inner.skew_secs))
            .is_none()
        {
            return Err(invalid("clock skew too large"));
        }
        Ok(CoseEnvelope {
            inner: Arc::new(inner),
        })
    }
}

fn invalid(message: &str) -> CratestackError {
    CratestackError::Validation(message.to_owned())
}

/// Shows the configuration, not the key material: signers expose only
/// their algorithm and `kid`, and the injected closures are opaque.
impl fmt::Debug for CoseEnvelopeBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner
            .debug_fields(&mut f.debug_struct("CoseEnvelopeBuilder"))
    }
}
