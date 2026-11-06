//! Signing a claim object.

use super::error::ClaimsError;
use super::header;
use super::types::CwtClaims;
use crate::alg::CoseMode;
use crate::cbor::write::{self, EMPTY_MAP, MAJOR_ARRAY, MAJOR_TAG};
use crate::keys::{CoseSigner, esp256_low_s};
use crate::tbs::Tbs;
use crate::thumbprint::KID_LEN;

/// Sign `payload` as a claim object: a tagged `COSE_Sign1` (tag 18) whose
/// protected header is `{1: alg, 4: kid, ? 15: claims, 16: typ}`, whose
/// unprotected header is empty and whose payload is embedded.
///
/// - `typ` (RFC 9596, label 16) names what the object is. It is signed, and
///   [`open_claims`](crate::open_claims) refuses any other, so an object
///   minted for one purpose cannot be replayed as another. It is also what
///   keeps a claim object from opening as a transport envelope, and the
///   reverse. Non-empty, at most [`MAX_TYP_LEN`](crate::MAX_TYP_LEN) bytes.
/// - `claims` are written only when present; see [`CwtClaims`] for the
///   bounds.
/// - `external_aad` is signed but not sent: the opener must supply the same
///   bytes. Put in it what the object is bound to but does not carry, such
///   as a tenant or a purpose; empty is fine.
///
/// The encoding is deterministic (RFC 8949 §4.2.1) and an ESP256 signature
/// is low-`s`, the rules the transport envelope enforces. `signer` must be
/// a signature signer (Ed25519 or ESP256); an HMAC signer is refused with
/// [`ClaimsError::Misuse`]. Nothing is read from a clock: `exp` and `iat`
/// are whatever the caller put in.
///
/// ```
/// use cratestack_cose::{CwtClaims, Ed25519Signer, sign_claims};
///
/// # tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(async {
/// let signer = Ed25519Signer::from_seed(&[7; 32]);
/// let claims = CwtClaims { exp: Some(1_790_003_600), ..CwtClaims::default() };
/// let bytes = sign_claims(&signer, "application/vnd.example.grant+cwt", &claims, b"grant", b"")
///     .await
///     .unwrap();
/// assert_eq!(bytes[0], 0xd2); // tag 18
/// # });
/// ```
pub async fn sign_claims(
    signer: &dyn CoseSigner,
    typ: &str,
    claims: &CwtClaims,
    payload: &[u8],
    external_aad: &[u8],
) -> Result<Vec<u8>, ClaimsError> {
    let alg = signer.alg();
    if alg.mode() != CoseMode::Sign1 {
        return Err(ClaimsError::Misuse(
            "a claim object needs a signature algorithm, not a MAC",
        ));
    }
    if signer.kid().len() != KID_LEN {
        return Err(ClaimsError::Misuse("the signer's kid must be 8 bytes"));
    }
    if !header::typ_ok(typ) {
        return Err(ClaimsError::Misuse("typ must be 1 to MAX_TYP_LEN bytes"));
    }
    header::check_claims(claims).map_err(ClaimsError::Misuse)?;

    let protected = header::encode(alg, signer.kid(), typ, claims);
    let tbs = Tbs {
        mode: CoseMode::Sign1,
        protected: &protected,
        external_aad,
        payload,
    };
    let signature = match tbs.with_chunks(|chunks| signer.sign_chunks(chunks)) {
        Some(signed) => signed,
        None => signer.sign(&tbs.assemble()).await,
    }
    .map_err(ClaimsError::Signer)?;
    if signature.len() != alg.signature_len() {
        return Err(ClaimsError::Misuse(
            "the signer returned a signature of the wrong length",
        ));
    }
    let signature = if alg == crate::CoseAlg::Esp256 {
        esp256_low_s(&signature).ok_or(ClaimsError::Misuse(
            "the signer returned an invalid ESP256 signature",
        ))?
    } else {
        signature
    };

    let mut out = Vec::with_capacity(
        2 + write::bstr_len(protected.len())
            + 1
            + write::bstr_len(payload.len())
            + write::bstr_len(signature.len()),
    );
    write::head(&mut out, MAJOR_TAG, CoseMode::Sign1.tag());
    write::head(&mut out, MAJOR_ARRAY, 4);
    write::bstr(&mut out, &protected);
    out.push(EMPTY_MAP);
    write::bstr(&mut out, payload);
    write::bstr(&mut out, &signature);
    Ok(out)
}
