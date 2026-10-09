//! Sealing one call for a caller that does its own HTTP (cratestack#1168).
//!
//! [`ClientEnvelope::seal_call`] is the one place a request's binding is
//! assembled and sealed: `request_sealed` (the generated clients) runs on it,
//! and so can a Node SDK, a hand-written adapter or a Rust SDK that sends
//! the bytes with its own HTTP client. The caller sends
//! [`SealedCall::body`] with [`SealedCall::headers`] to the URL it chose,
//! then hands the answer to [`PendingResponse::open`].

use bytes::Bytes;
use cratestack_core::{
    BoundHeaders, CONTRACT_HEADER, ContractSelector, DEFAULT_PAYLOAD_MEDIA_TYPE,
    PAYLOAD_ACCEPT_HEADER, PAYLOAD_TYPE_HEADER, canonical_query, is_sealable_payload_type,
    parse_payload_accept,
};
use cratestack_cose::request_digest;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderName, HeaderValue};

use super::{ClientEnvelope, PendingResponse, SealCall};
use crate::client::bound_headers::check_bound_value;
use crate::envelope_error::EnvelopeError;
use crate::error::ClientError;

/// A sealed call, ready to send.
#[derive(Debug)]
#[non_exhaustive]
pub struct SealedCall {
    /// The request body: the COSE message.
    pub body: Bytes,
    /// Every header the seal depends on, to send as given: the envelope's
    /// `Content-Type` and `Accept`, `Cratestack-Contract`, the payload-type
    /// selectors when they are not the CBOR default, and the bound
    /// `Idempotency-Key` / `If-Match`. Anything else (`Authorization`-style
    /// headers, a tracing id) is the caller's to add.
    pub headers: Vec<(HeaderName, HeaderValue)>,
    /// What the answer is opened with.
    pub pending: PendingResponse,
}

impl ClientEnvelope {
    /// Seal `call` without sending it. Fails with `BadInput`, signing
    /// nothing, for a payload or accept type that cannot be sealed (see
    /// [`SealCall::payload`]), a bound header that would not survive a hop
    /// untouched, or a `/rpc/batch` (route `"batch"`) that is not CBOR both
    /// ways; with [`EnvelopeError::Seal`] when the signer fails.
    ///
    /// ```no_run
    /// # async fn demo(envelope: cratestack_client_rust::ClientEnvelope) -> Result<(), Box<dyn std::error::Error>> {
    /// use cratestack_client_rust::{RouteRef, SealCall};
    ///
    /// let route = RouteRef::new("/charges", &[]);
    /// let sealed = envelope
    ///     .seal_call(SealCall::new("POST", route, [7; 32]).payload(b"\xa0", "application/cbor"))
    ///     .await?;
    /// let mut request = reqwest::Client::new().post("https://pay.example/charges").body(sealed.body);
    /// for (name, value) in &sealed.headers {
    ///     request = request.header(name, value);
    /// }
    /// let response = request.send().await?;
    /// let (status, headers) = (response.status().as_u16(), response.headers().clone());
    /// let opened = sealed.pending.open(status, &headers, response.bytes().await?).await?;
    /// println!("{} bytes of {}", opened.body.len(), opened.payload_type);
    /// # Ok(()) }
    /// ```
    pub async fn seal_call(&self, call: SealCall<'_>) -> Result<SealedCall, ClientError> {
        let accept = check_payload_types(call.payload_type, call.payload_accept)?;
        if call.route.template() == "batch"
            && (call.payload_type != DEFAULT_PAYLOAD_MEDIA_TYPE
                || !accept.contains(&DEFAULT_PAYLOAD_MEDIA_TYPE))
        {
            return Err(ClientError::BadInput(
                "a sealed /rpc/batch is CBOR both ways: its frames are".to_owned(),
            ));
        }
        for (name, value) in [
            ("Idempotency-Key", call.idempotency_key),
            ("If-Match", call.if_match),
        ] {
            if let Some(value) = value {
                check_bound_value(name, value)?;
            }
        }
        let bound = BoundHeaders {
            idempotency_key: call.idempotency_key.map(|key| key.to_owned().into()),
            if_match: call.if_match.map(|value| value.to_owned().into()),
        };
        let query = Some(canonical_query(call.query)).filter(|query| !query.is_empty());
        let binding = self.binding(
            call.method,
            call.route.template(),
            call.route.params(),
            query,
            call.contract_sha,
            bound,
            call.payload_type,
        );
        let sealed = self
            .cose()
            .seal_request(call.payload, &binding)
            .await
            .map_err(|error| ClientError::from(EnvelopeError::Seal(error)))?;
        let headers = headers(self.media_type(), &call)?;
        // The response binds the digest of exactly these bytes.
        // The pending answer is handed to the caller and outlives `&self`, so
        // it owns a copy of the envelope: the COSE handle inside is shared,
        // the audience and mount values are a few short strings.
        let pending = PendingResponse::new(
            self.clone(),
            binding.into_owned(),
            request_digest(&sealed),
            accept.into_iter().map(str::to_owned).collect(),
        );
        Ok(SealedCall {
            body: sealed,
            headers,
            pending,
        })
    }
}

/// The payload type and the accept list of a call, checked: each must be
/// sealable (`cratestack_core::is_sealable_payload_type`), and the list well
/// formed. Returns the accept list.
pub(crate) fn check_payload_types<'a>(
    payload_type: &str,
    payload_accept: &'a str,
) -> Result<Vec<&'a str>, ClientError> {
    if !is_sealable_payload_type(payload_type) {
        return Err(ClientError::BadInput(format!(
            "a COSE envelope cannot wrap a payload of type {payload_type:?}: a payload type is a \
             lowercase type/subtype without parameters, and never an envelope, a stream or a \
             multipart body"
        )));
    }
    let accept = parse_payload_accept(payload_accept.as_bytes())
        .map_err(|error| ClientError::BadInput(error.to_string()))?;
    match accept.iter().find(|entry| !is_sealable_payload_type(entry)) {
        Some(entry) => Err(ClientError::BadInput(format!(
            "a COSE envelope cannot carry an answer of type {entry:?}"
        ))),
        None => Ok(accept),
    }
}

fn headers(
    media_type: &'static str,
    call: &SealCall<'_>,
) -> Result<Vec<(HeaderName, HeaderValue)>, ClientError> {
    let value = |text: &str| {
        HeaderValue::from_str(text)
            .map_err(|error| ClientError::BadInput(format!("invalid header value: {error}")))
    };
    let name = |text: &str| {
        HeaderName::from_bytes(text.as_bytes())
            .map_err(|error| ClientError::BadInput(format!("invalid header name: {error}")))
    };
    let selector = ContractSelector::of(&call.contract_sha).to_header_value();
    let mut headers = vec![
        (CONTENT_TYPE, HeaderValue::from_static(media_type)),
        (ACCEPT, HeaderValue::from_static(media_type)),
        (name(CONTRACT_HEADER)?, value(&selector)?),
    ];
    // CBOR is what an absent header means, so a CBOR call sends neither and
    // sends the request headers 0.15.3 sent (the response gains one).
    if call.payload_type != DEFAULT_PAYLOAD_MEDIA_TYPE {
        headers.push((name(PAYLOAD_TYPE_HEADER)?, value(call.payload_type)?));
    }
    if call.payload_accept != DEFAULT_PAYLOAD_MEDIA_TYPE {
        headers.push((name(PAYLOAD_ACCEPT_HEADER)?, value(call.payload_accept)?));
    }
    for (header, bound) in [
        ("idempotency-key", call.idempotency_key),
        ("if-match", call.if_match),
    ] {
        if let Some(bound) = bound {
            headers.push((name(header)?, value(bound)?));
        }
    }
    Ok(headers)
}
