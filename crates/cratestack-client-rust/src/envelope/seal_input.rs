//! What one sealed call is made of (cratestack#1168).

use cratestack_core::DEFAULT_PAYLOAD_MEDIA_TYPE;

use crate::client::route::RouteRef;

/// One call to seal. Build it with [`SealCall::new`] and the chainable
/// setters; everything but the method, route and contract digest defaults to
/// an empty CBOR payload answered in CBOR, which is what a generated client
/// sends.
///
/// ```
/// use cratestack_client_rust::{RouteRef, SealCall};
///
/// let route = RouteRef::new("/charges/{id}", &["ch_1"]);
/// let call = SealCall::new("POST", route, [7; 32])
///     .query(Some("expand=customer"))
///     .payload(b"amount=1500", "application/x-www-form-urlencoded")
///     .accept("application/json")
///     .idempotency_key("idem-1");
/// # drop(call);
/// ```
#[derive(Debug, Clone)]
pub struct SealCall<'a> {
    pub(super) method: &'a str,
    pub(super) route: RouteRef<'a>,
    pub(super) query: Option<&'a str>,
    pub(super) contract_sha: [u8; 32],
    pub(super) payload: &'a [u8],
    pub(super) payload_type: &'a str,
    pub(super) payload_accept: &'a str,
    pub(super) idempotency_key: Option<&'a str>,
    pub(super) if_match: Option<&'a str>,
}

impl<'a> SealCall<'a> {
    /// A call to `route` with `method` (`"POST"`), bound under the op
    /// contract digest `contract_sha`.
    pub fn new(method: &'a str, route: RouteRef<'a>, contract_sha: [u8; 32]) -> Self {
        Self {
            method,
            route,
            query: None,
            contract_sha,
            payload: &[],
            payload_type: DEFAULT_PAYLOAD_MEDIA_TYPE,
            payload_accept: DEFAULT_PAYLOAD_MEDIA_TYPE,
            idempotency_key: None,
            if_match: None,
        }
    }

    /// The query string as it is sent (the seal binds its canonical form).
    #[must_use]
    pub fn query(mut self, query: Option<&'a str>) -> Self {
        self.query = query;
        self
    }

    /// The request payload and its type (a lowercase `type/subtype`, never an
    /// envelope or a stream). An empty payload is a bodiless request.
    #[must_use]
    pub fn payload(mut self, payload: &'a [u8], payload_type: &'a str) -> Self {
        self.payload = payload;
        self.payload_type = payload_type;
        self
    }

    /// The response payload types this call reads, as the
    /// `Cratestack-Payload-Accept` value: one to eight types joined by
    /// `", "`, in order of preference. The default is `application/cbor`.
    #[must_use]
    pub fn accept(mut self, payload_accept: &'a str) -> Self {
        self.payload_accept = payload_accept;
        self
    }

    /// The `Idempotency-Key` the request is sent with, which the seal binds.
    #[must_use]
    pub fn idempotency_key(mut self, key: &'a str) -> Self {
        self.idempotency_key = Some(key);
        self
    }

    /// The `If-Match` the request is sent with, which the seal binds.
    #[must_use]
    pub fn if_match(mut self, if_match: &'a str) -> Self {
        self.if_match = Some(if_match);
        self
    }
}
