//! [`BoundHeaders`]: the request headers with semantics that a
//! [`Binding`](super::Binding) authenticates (ADR 0006 §4, as amended by the
//! maintainer's decision S1 after the cratestack#1006 security review,
//! 2026-09-26).

use std::borrow::Cow;

/// The two request headers that change what a request *does*, bound into
/// the AAD as `bound_headers: [idempotency_key: tstr / null, if_match:
/// tstr / null]`, right after `payload_type`.
///
/// Without them an on-path party could strip or swap them on a signed
/// request without breaking its signature: dropping `Idempotency-Key` from
/// a client's re-sealed retry makes the server run a payment twice, and
/// dropping or changing `If-Match` turns an optimistic-locking update into
/// a blind overwrite. Each is `None` when the request does not carry it.
///
/// The value is bound **exactly as the header carried it**, as UTF-8, with
/// no trimming or case folding: the server's envelope layer reads the one
/// header value it received, and a request carrying the header twice is
/// refused before anything is bound (which one a client meant is not the
/// server's to guess). A proxy between the two must therefore forward
/// these headers byte for byte.
///
/// Response headers (`ETag`, `Retry-After`, ...) stay unauthenticated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoundHeaders<'a> {
    /// The `Idempotency-Key` request header.
    pub idempotency_key: Option<Cow<'a, str>>,
    /// The `If-Match` request header.
    pub if_match: Option<Cow<'a, str>>,
}

impl BoundHeaders<'_> {
    /// Neither header. What every request that carries neither binds, and
    /// what `Default` returns; it allocates nothing.
    pub const NONE: BoundHeaders<'static> = BoundHeaders {
        idempotency_key: None,
        if_match: None,
    };

    /// Detach from the borrowed request data (see
    /// [`Binding::into_owned`](super::Binding::into_owned)).
    pub fn into_owned(self) -> BoundHeaders<'static> {
        BoundHeaders {
            idempotency_key: self
                .idempotency_key
                .map(|value| Cow::Owned(value.into_owned())),
            if_match: self.if_match.map(|value| Cow::Owned(value.into_owned())),
        }
    }
}
