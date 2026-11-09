//! The napi class of a seal that waits for its external signer, and the
//! contract header helper.

use napi::bindgen_prelude::Uint8Array;
use napi_derive::napi;

use super::external::Suspended;
use super::fail::Fail;

fn failure(fail: Fail) -> napi::Error<String> {
    napi::Error::new(fail.code.to_owned(), fail.message)
}

/// A seal that waits for the external signer's signature.
#[napi]
pub struct PendingSeal {
    pub(super) inner: Suspended,
}

#[napi]
impl PendingSeal {
    /// What to sign: the complete COSE `Sig_structure`. Sign exactly these
    /// bytes, never a digest of them.
    #[napi(getter)]
    pub fn to_be_signed(&self) -> Uint8Array {
        Uint8Array::from(self.inner.to_be_signed().to_vec())
    }

    /// Finish the seal with the signature (DER or raw `r ‖ s` for ESP256;
    /// 64 bytes for Ed25519). Resolves to the sealed bytes.
    #[napi(catch_unwind)]
    pub fn finish(&self, signature: Uint8Array) -> napi::Result<Uint8Array, String> {
        self.inner
            .finish(signature.to_vec())
            .map(Uint8Array::from)
            .map_err(failure)
    }
}

/// The `Cratestack-Contract` header value for a 32-byte op contract digest.
#[napi(catch_unwind)]
pub fn contract_header_value(contract_sha: Uint8Array) -> napi::Result<String, String> {
    let digest: [u8; 32] = contract_sha
        .as_ref()
        .try_into()
        .map_err(|_| failure(Fail::misuse("`contractSha` is 32 bytes")))?;
    Ok(cratestack_cose::contract_header_value(&digest))
}

/// The canonical form of a raw query string (the part after `?`, without
/// it): the form `binding.query` must carry for the seal to match what the
/// server rebuilds. By `cratestack_core::canonical_query`, so no client
/// re-implements it.
#[napi]
pub fn canonical_query(query: String) -> String {
    cratestack_core::canonical_query(Some(&query))
}
