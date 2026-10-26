//! Opening a claim object.

use super::error::ClaimsError;
use super::header;
use super::types::OpenedClaims;
use crate::alg::CoseMode;
use crate::error::Reject;
use crate::keys::{CoseVerifierResolver, CoseVerifyKey};
use crate::tbs::Tbs;
use crate::thumbprint::KID_LEN;
use crate::wire;

/// Open a claim object: parse it strictly, check its `typ`, and verify its
/// signature with the key `resolver` returns for its `kid`.
///
/// **This checks authenticity, nothing else.** There is no skew window and
/// no nonce store here, by design: a claim object is signed by one party
/// and may be delivered much later by another, so a transport envelope's
/// ±[`DEFAULT_SKEW_SECS`](crate::DEFAULT_SKEW_SECS) freshness would reject
/// it. Whether `exp` has passed, whether `aud` is this service and whether
/// the `cti` was already accepted are the caller's to enforce on the
/// returned [`claims`](OpenedClaims::claims). An object with no `exp` never
/// expires, and the same bytes open as many times as they are presented.
///
/// Anything but an exact match of this profile is [`ClaimsError::Malformed`]
/// (checked before any key is looked up): an untagged or non-`COSE_Sign1`
/// message, a non-minimal or indefinite CBOR head, an unknown, duplicate or
/// out-of-order header label, an unprotected header entry, a detached
/// payload, a MAC algorithm, a header with no `typ` (which is what a
/// transport envelope looks like) and trailing bytes. A different `typ` is
/// [`ClaimsError::WrongType`]; an unknown `kid` or a bad signature is
/// [`ClaimsError::Unverified`]. `external_aad` must equal the bytes given
/// to [`sign_claims`](crate::sign_claims). ESP256 accepts low-`s` only.
///
/// ```
/// use cratestack_cose::{
///     ClaimsError, CwtClaims, Ed25519Signer, StaticVerifierResolver, open_claims, sign_claims,
/// };
///
/// # tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(async {
/// const TYP: &str = "application/vnd.example.grant+cwt";
/// let signer = Ed25519Signer::from_seed(&[7; 32]);
/// let resolver = StaticVerifierResolver::new().with_key(signer.verify_key());
/// let claims = CwtClaims { exp: Some(1_790_003_600), ..CwtClaims::default() };
/// let bytes = sign_claims(&signer, TYP, &claims, b"grant", b"tenant-1").await.unwrap();
///
/// let opened = open_claims(&resolver, &bytes, TYP, b"tenant-1").await.unwrap();
/// assert_eq!(opened.payload, b"grant");
/// // Still verifies years later: expiry is the caller's check.
/// assert_eq!(opened.claims.exp, Some(1_790_003_600));
///
/// let wrong_aad = open_claims(&resolver, &bytes, TYP, b"tenant-2").await;
/// assert!(matches!(wrong_aad, Err(ClaimsError::Unverified)));
/// # });
/// ```
pub async fn open_claims(
    resolver: &dyn CoseVerifierResolver,
    bytes: &[u8],
    expected_typ: &str,
    external_aad: &[u8],
) -> Result<OpenedClaims, ClaimsError> {
    let malformed = |_: Reject| ClaimsError::Malformed;
    let parts = wire::parse(CoseMode::Sign1, bytes).map_err(malformed)?;
    let protected_bytes = &bytes[parts.protected];
    let parsed = header::parse(protected_bytes).map_err(malformed)?;
    if parsed.alg.mode() != CoseMode::Sign1 {
        return Err(ClaimsError::Malformed);
    }
    if parsed.typ != expected_typ {
        return Err(ClaimsError::WrongType);
    }
    let kid: [u8; KID_LEN] = protected_bytes[parsed.kid]
        .try_into()
        .map_err(|_| ClaimsError::Malformed)?;

    let candidates = resolver
        .resolve(&kid, parsed.alg)
        .await
        .map_err(ClaimsError::Resolver)?;
    let payload = &bytes[parts.payload];
    let signature = &bytes[parts.signature];
    let tbs = Tbs {
        mode: CoseMode::Sign1,
        protected: protected_bytes,
        external_aad,
        payload,
    };
    // Candidates are filtered on the key's own `kid`, as the envelope does,
    // so a resolver that over-returns cannot let a key sign under another's.
    let key = candidates
        .iter()
        .find(|key| key.kid() == kid && key.verify(parsed.alg, &tbs, signature))
        .ok_or(ClaimsError::Unverified)?;
    Ok(OpenedClaims {
        kid,
        alg: parsed.alg,
        key_thumbprint: CoseVerifyKey::thumbprint(key),
        claims: parsed.claims,
        payload: payload.to_vec(),
    })
}
