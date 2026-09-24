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
//! 4. verify over the **received** protected bytes, trying each candidate;
//!    a candidate of the wrong key type never verifies;
//! 5. requests: `iat` within the skew;
//! 6. requests: record `(kid, cti)` in the nonce store.
//!
//! The nonce is recorded last, only for a message that passed everything
//! else. Recording earlier would let anyone burn a legitimate client's
//! `cti` with a forged message that fails verification.

use bytes::Bytes;
use cratestack_core::{Binding, CratestackError};

use crate::aad::{self, Direction};
use crate::envelope::Inner;
use crate::error::{Reject, backend, misuse};
use crate::opened::Opened;
use crate::replay;
use crate::{header, wire};

pub(crate) async fn open(
    inner: &Inner,
    body: Bytes,
    bind: &Binding<'_>,
    request: bool,
) -> Result<Opened, CratestackError> {
    let direction = aad::direction(bind)?;
    if request != matches!(direction, Direction::Request) {
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

    let parts = wire::parse(inner.mode, &body)?;
    let protected_bytes = &body[parts.protected.clone()];
    let protected = header::parse(protected_bytes)?;
    if protected.alg.mode() != inner.mode || protected.claims.is_some() != request {
        return Err(Reject.into());
    }
    let base = parts.protected.start;
    let kid = body.slice(base + protected.kid.start..base + protected.kid.end);

    let candidates = inner
        .resolver
        .resolve(&kid, protected.alg)
        .await
        .map_err(|error| backend("key resolver", error))?;
    let to_be_signed = wire::to_be_signed(
        inner.mode,
        protected_bytes,
        &external_aad,
        &body[parts.payload.clone()],
    );
    let signature = &body[parts.signature.clone()];
    let key = candidates
        .iter()
        .find(|key| key.verify(protected.alg, &to_be_signed, signature))
        .ok_or(Reject)?;

    let (iat, cti) = match (protected.claims, nonce_store) {
        (Some((iat, cti)), Some(store)) => {
            if !replay::is_fresh((inner.clock)(), iat, inner.skew_secs) {
                return Err(Reject.into());
            }
            let cti = body.slice(base + cti.start..base + cti.end);
            let expires_at = replay::nonce_expiry(iat, inner.skew_secs).ok_or(Reject)?;
            let first = store
                .record_if_unseen(&replay::nonce_key(&kid, &cti), expires_at)
                .await
                .map_err(|error| backend("nonce store", error))?;
            if !first {
                return Err(Reject.into());
            }
            (Some(iat), Some(cti))
        }
        _ => (None, None),
    };

    Ok(Opened {
        payload: body.slice(parts.payload),
        kid,
        alg: protected.alg,
        key_thumbprint: key.thumbprint(),
        iat,
        cti,
    })
}
