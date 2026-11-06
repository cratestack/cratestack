//! Conversions from the bridge's value types to `cratestack-cose`'s, kept
//! out of `types.rs` so that file holds only what crosses the bridge.

use cratestack_core::CratestackError;
use cratestack_cose::{CallBinding, CoseAlg, CoseVerifyKey, Opened};

use super::error::FlutterCoseError;
use super::types::{FlutterCallBinding, FlutterOpened, FlutterServerKey};

impl FlutterOpened {
    /// `opened`, reported with the `payload_type` it verified under.
    pub(crate) fn new(opened: Opened, payload_type: &str) -> Result<Self, FlutterCoseError> {
        Ok(Self {
            payload_type: payload_type.to_owned(),
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
    pub(crate) fn with_audience(self, audience: &str) -> Result<CallBinding, FlutterCoseError> {
        let contract_sha = self
            .contract_sha
            .as_slice()
            .try_into()
            .map_err(|_| FlutterCoseError::misuse("the op contract digest is 32 bytes"))?;
        let call = CallBinding {
            audience: audience.to_owned(),
            method: self.method,
            route: self.route,
            path_params: self.path_params,
            query: self.query,
            contract_sha,
            idempotency_key: self.idempotency_key,
            if_match: self.if_match,
            payload_type: self.payload_type,
            payload_accept: self.payload_accept,
        };
        call.validate()?;
        Ok(call)
    }
}
