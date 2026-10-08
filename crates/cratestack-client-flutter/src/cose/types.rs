//! The value types of the COSE bridge, and their mapping to
//! `cratestack-cose`'s.

use cratestack_core::CratestackError;
use cratestack_cose::{CallBinding, CoseAlg, CoseMode, CoseVerifyKey, Opened};

use super::error::FlutterCoseError;

/// The four algorithms of ADR 0006 §2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlutterCoseAlg {
    /// Ed25519 (`-19`), Sign1.
    Ed25519,
    /// ESP256 (`-9`), Sign1, low-`s` only.
    Esp256,
    /// HMAC 256/64 (`4`), Mac0.
    Hmac256_64,
    /// HMAC 256/256 (`5`), Mac0.
    Hmac256_256,
}

/// The message structure an envelope emits and accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlutterCoseMode {
    /// COSE_Sign1 (tag 18), `application/cose; cose-type="cose-sign1"`.
    Sign1,
    /// COSE_Mac0 (tag 17), `application/cose; cose-type="cose-mac0"`.
    Mac0,
}

/// A key the server's responses verify with, pinned at enrolment.
///
/// `Debug` shows the algorithm and the length of `bytes`, never `bytes`:
/// for a COSE_Mac0 server it holds the shared secret.
#[derive(Clone, PartialEq, Eq)]
pub struct FlutterServerKey {
    /// The algorithm this key verifies, and only it.
    pub alg: FlutterCoseAlg,
    /// Ed25519: the 32-byte public key. ESP256: the SEC1 point
    /// (compressed or uncompressed). HMAC: the shared secret.
    pub bytes: Vec<u8>,
}

/// The inputs of one call's binding. The audience is the envelope's, not
/// the call's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlutterCallBinding {
    /// HTTP method, e.g. `"POST"`.
    pub method: String,
    /// The RPC `op_id`, or the REST route template.
    pub route: String,
    /// REST path parameter values in template order; empty for RPC.
    pub path_params: Vec<String>,
    /// The query string in any spelling; it is canonicalised.
    pub query: Option<String>,
    /// The op contract digest, 32 bytes. A `Vec` and not `[u8; 32]`:
    /// `flutter_rust_bridge` renders an array as a generated class that
    /// needs `package:collection`, which this package does not depend on.
    pub contract_sha: Vec<u8>,
    /// The `Idempotency-Key` the request will carry, if any.
    pub idempotency_key: Option<String>,
    /// The `If-Match` the request will carry, if any.
    pub if_match: Option<String>,
}

/// A response that verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlutterOpened {
    /// The payload, exactly as signed.
    pub payload: Vec<u8>,
    /// The signer's 8-byte `kid`.
    pub kid: Vec<u8>,
    /// The algorithm it verified with.
    pub alg: FlutterCoseAlg,
    /// The RFC 9679 thumbprint of the key that verified (32 bytes).
    pub thumbprint: Vec<u8>,
}

/// Pins `iat` and `cti` so the shared vectors reproduce. **For tests
/// only**: a fixed `cti` is a replayed request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlutterSealOptions {
    /// `iat` in Unix seconds, instead of the clock.
    pub fixed_iat: Option<i64>,
    /// `cti` (1 to 4 or 16 bytes), instead of random bytes.
    pub fixed_cti: Option<Vec<u8>>,
}

/// `CoseAlg` is `#[non_exhaustive]` (a reserved hybrid algorithm), so a
/// value this bridge has no name for is misuse, never a guess.
impl std::fmt::Debug for FlutterServerKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlutterServerKey")
            .field("alg", &self.alg)
            .field("bytes", &format_args!("<{} bytes>", self.bytes.len()))
            .finish()
    }
}

impl TryFrom<CoseAlg> for FlutterCoseAlg {
    type Error = FlutterCoseError;

    fn try_from(alg: CoseAlg) -> Result<Self, Self::Error> {
        match alg {
            CoseAlg::Ed25519 => Ok(Self::Ed25519),
            CoseAlg::Esp256 => Ok(Self::Esp256),
            CoseAlg::Hmac256_64 => Ok(Self::Hmac256_64),
            CoseAlg::Hmac256_256 => Ok(Self::Hmac256_256),
            _ => Err(FlutterCoseError::misuse(
                "an algorithm this bridge has no name for",
            )),
        }
    }
}

impl From<FlutterCoseAlg> for CoseAlg {
    fn from(alg: FlutterCoseAlg) -> Self {
        match alg {
            FlutterCoseAlg::Ed25519 => Self::Ed25519,
            FlutterCoseAlg::Esp256 => Self::Esp256,
            FlutterCoseAlg::Hmac256_64 => Self::Hmac256_64,
            FlutterCoseAlg::Hmac256_256 => Self::Hmac256_256,
        }
    }
}

impl From<CoseMode> for FlutterCoseMode {
    fn from(mode: CoseMode) -> Self {
        match mode {
            CoseMode::Sign1 => Self::Sign1,
            CoseMode::Mac0 => Self::Mac0,
        }
    }
}

impl TryFrom<Opened> for FlutterOpened {
    type Error = FlutterCoseError;

    fn try_from(opened: Opened) -> Result<Self, Self::Error> {
        Ok(Self {
            payload: opened.payload.to_vec(),
            kid: opened.kid.to_vec(),
            alg: opened.alg.try_into()?,
            thumbprint: opened.thumbprint.to_vec(),
        })
    }
}

impl FlutterServerKey {
    /// The typed verification key; a key of the wrong shape for its
    /// algorithm is misuse.
    pub(crate) fn to_verify_key(&self) -> Result<CoseVerifyKey, FlutterCoseError> {
        let alg = CoseAlg::from(self.alg);
        let key = match alg {
            CoseAlg::Ed25519 => {
                let public: [u8; 32] = self.bytes.as_slice().try_into().map_err(|_| {
                    CratestackError::Validation("an Ed25519 server key is 32 bytes".to_owned())
                })?;
                CoseVerifyKey::ed25519(&public)
            }
            CoseAlg::Esp256 => CoseVerifyKey::p256_sec1(&self.bytes),
            _ => CoseVerifyKey::hmac(alg, self.bytes.clone()),
        };
        Ok(key?)
    }
}

impl FlutterCallBinding {
    /// The owned binding, addressed to `audience`.
    pub(crate) fn with_audience(&self, audience: &str) -> Result<CallBinding, FlutterCoseError> {
        let contract_sha = self
            .contract_sha
            .as_slice()
            .try_into()
            .map_err(|_| FlutterCoseError::misuse("the op contract digest is 32 bytes"))?;
        Ok(CallBinding {
            audience: audience.to_owned(),
            method: self.method.clone(),
            route: self.route.clone(),
            path_params: self.path_params.clone(),
            query: self.query.clone(),
            contract_sha,
            idempotency_key: self.idempotency_key.clone(),
            if_match: self.if_match.clone(),
        })
    }
}
