//! Sealing: payload in, COSE message out.

use bytes::Bytes;
use cratestack_core::{Binding, CratestackError};

use crate::aad::{self, Direction};
use crate::envelope::Inner;
use crate::error::{backend, misuse};
use crate::header;
use crate::wire;

/// Seal `payload` for `bind`, as a request (`request == true`, with `iat`
/// and `cti`) or a response (`kid` and `alg` only; Q1).
///
/// **Deviation from ADR 0006 §1** ("the sealer encodes straight into the
/// COSE buffer ... then patches the head"): `CratestackEnvelope::seal`
/// receives the codec's output as `Bytes`, already encoded, and the trait
/// has no encode-in-place hook. The payload is therefore copied into the
/// message once, and once more into the to-be-signed structure (see
/// `wire::to_be_signed`). The bytes on the wire are the codec's output
/// unchanged, which is what the "no re-serialization" requirement protects;
/// only the zero-copy half of §1 is given up.
pub(crate) async fn seal(
    inner: &Inner,
    payload: &[u8],
    bind: &Binding<'_>,
    request: bool,
) -> Result<Bytes, CratestackError> {
    let direction = aad::direction(bind)?;
    if request != matches!(direction, Direction::Request) {
        return Err(misuse(if request {
            "sealing a request with a response binding"
        } else {
            "sealing a response with a request binding"
        }));
    }
    let signer = inner.signer.as_ref();
    let alg = signer.alg();
    let cti;
    let claims = if request {
        let iat = u64::try_from((inner.clock)())
            .map_err(|_| misuse("the clock returned a time before 1970"))?;
        cti = (inner.cti)()?;
        if !header::cti_len_ok(cti.len()) {
            return Err(misuse("the cti source must return 1 to 4 or 16 bytes"));
        }
        Some((iat, cti.as_slice()))
    } else {
        None
    };
    let protected = header::encode(alg, signer.kid(), claims);
    let external_aad = aad::external_aad(bind)?;
    let to_be_signed = wire::to_be_signed(inner.mode, &protected, &external_aad, payload);
    let signature = signer
        .sign(&to_be_signed)
        .await
        .map_err(|error| backend("signer", error))?;
    if signature.len() != alg.signature_len() {
        return Err(misuse(
            "the signer returned a signature of the wrong length",
        ));
    }
    Ok(Bytes::from(wire::emit(
        inner.mode, &protected, payload, &signature,
    )))
}
