//! Opening: COSE message in, verified payload out.
//!
//! The checks run in a fixed order, cheapest and least trusting first, and
//! every one of them fails with the same coarse `401`:
//!
//! 1. strict parse of the outer message, which includes the tag
//!    (`wire::parse`);
//! 2. strict parse of the protected header, whose `alg` must be on the
//!    allowlist (`header::parse`), belong to the envelope's mode (so the
//!    tag and the algorithm agree), and carry claims exactly when a request
//!    is expected;
//! 3. resolve candidate keys by `(kid, alg)`;
//! 4. verify over the **received** protected bytes, trying each candidate
//!    whose own `kid` (computed from the key, never taken from the
//!    resolver's word) is the header's `kid` and whose algorithm is the
//!    header's `alg`. The principal is the key that verified, so a resolver
//!    that returns too much (every key of a tenant, say) cannot let one key
//!    sign under another's `kid`;
//! 5. requests: `iat` within the skew;
//! 6. requests: record `(kid, cti)` in the nonce store.
//!
//! The nonce is recorded last, only for a message that passed everything
//! else. Recording earlier would let anyone burn a legitimate client's
//! `cti` with a forged message that fails verification.

use bytes::Bytes;
use cratestack_core::{Binding, CratestackError};

use crate::aad;
use crate::envelope::Inner;
use crate::error::{Reject, backend, misuse};
use crate::keys::CoseVerifyKey;
use crate::opened::Opened;
use crate::replay;
use crate::tbs::Tbs;
use crate::thumbprint::KID_LEN;
use crate::{header, wire};

pub(crate) async fn open(
    inner: &Inner,
    body: Bytes,
    bind: &Binding<'_>,
    request: bool,
) -> Result<Opened, CratestackError> {
    if request == bind.response.is_some() {
        return Err(misuse(if request {
            "opening a request with a response binding"
        } else {
            "opening a response with a request binding"
        }));
    }
    let nonce_store = match (&inner.nonce_store, request) {
        (Some(store), true) => Some(store),
        (None, true) => return Err(misuse("opening a request needs a nonce store")),
        (_, false) => None,
    };
    let external_aad = aad::external_aad(bind)?;

    let parts = wire::parse(inner.mode, &body).map_err(Reject::into_error)?;
    let protected_bytes = &body[parts.protected.clone()];
    let protected = header::parse(protected_bytes).map_err(Reject::into_error)?;
    if protected.alg.mode() != inner.mode || protected.claims.is_some() != request {
        return Err(Reject.into_error());
    }
    let base = parts.protected.start;
    // A copy, not a slice of `body`: it outlives the call in `Opened` and
    // in the context's `VerifiedSigner`, and a slice would keep the whole
    // body alive with it. `header::parse` has checked it is 8 bytes.
    let kid: [u8; KID_LEN] = protected_bytes[protected.kid.clone()]
        .try_into()
        .map_err(|_| Reject.into_error())?;

    let candidates = inner
        .resolver
        .resolve(&kid, protected.alg)
        .await
        .map_err(|error| backend("key resolver", error))?;
    let signature = &body[parts.signature.clone()];
    let tbs = Tbs {
        mode: inner.mode,
        protected: protected_bytes,
        external_aad: &external_aad,
        payload: &body[parts.payload.clone()],
    };
    let thumbprint = candidates
        .iter()
        .find(|key| key.kid() == kid && key.verify(protected.alg, &tbs, signature))
        .map(CoseVerifyKey::thumbprint)
        .ok_or_else(|| Reject.into_error())?;

    let (iat, cti) = match (protected.claims, nonce_store) {
        (Some((iat, cti)), Some(store)) => {
            if !replay::is_fresh((inner.clock)(), iat, inner.skew_secs) {
                return Err(Reject.into_error());
            }
            let cti = body.slice(base + cti.start..base + cti.end);
            let expires_at =
                replay::nonce_expiry(iat, inner.skew_secs).ok_or_else(|| Reject.into_error())?;
            let first = store
                .record_if_unseen(&replay::nonce_key(&kid, &cti), expires_at)
                .await
                .map_err(|error| backend("nonce store", error))?;
            if !first {
                return Err(Reject.into_error());
            }
            (Some(iat), Some(cti))
        }
        _ => (None, None),
    };

    Ok(Opened {
        payload: body.slice(parts.payload),
        kid,
        alg: protected.alg,
        thumbprint,
        iat,
        cti,
    })
}
