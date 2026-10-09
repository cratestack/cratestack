//! One sealed call: seal the request, send it, open the response
//! (ADR 0006, cratestack#1007).
//!
//! The sealing and the opening are [`ClientEnvelope::seal_call`] and
//! [`PendingResponse::open`](crate::PendingResponse::open), the same pair a
//! caller doing its own HTTP uses: this file is the transport around them.
//!
//! The result is a [`RuntimeResponseWire`] whose body is the *opened*
//! payload and whose `Content-Type` is the type it was sealed under (CBOR
//! unless the codec says otherwise), so everything
//! downstream of `request_raw_with_query_and_accept` (typed decoding, the
//! `Remote` error mapping, cbor-seq lists, RPC frames) is the code a plain
//! call runs, with no second path to keep in step with it.

use cratestack_core::PAYLOAD_TYPE_HEADER;
use reqwest::Method;
use reqwest::StatusCode;
use reqwest::header::{
    CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, TRANSFER_ENCODING,
};

use crate::client::bound_headers::{bound_headers, refuse_duplicates};
use crate::client::core::CratestackClient;
use crate::client::envelope_refusal::refused_op;
use crate::client::helpers::{build_url, headers_to_runtime};
use crate::client::route::RouteRef;
use crate::codec::HttpClientCodec;
use crate::envelope::{ClientEnvelope, SealCall, SealedCall, is_cose};
use crate::envelope_error::EnvelopeError;
use crate::error::{ClientError, HeaderPair};
use crate::idempotency::RequestIdempotency;
use crate::runtime::wire::{RuntimeHeader, RuntimeResponseWire};

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    pub(crate) async fn request_sealed(
        &self,
        envelope: &ClientEnvelope,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        canonical: Option<&str>,
        headers: &[HeaderPair<'_>],
    ) -> Result<RuntimeResponseWire, ClientError> {
        let route = self.sealing.route.as_ref().ok_or_else(|| {
            ClientError::BadInput(
                "a sealed call needs its route: use the generated client, or `at(RouteRef)`"
                    .to_owned(),
            )
        })?;
        let (contract_sha, _) = self.contract_for(method.as_str(), &route.template)?;
        refuse_duplicates(headers)?;
        let url = build_url(&self.config.base_url, path, canonical)?;
        // The authorizer runs over the inner payload, exactly what the
        // server's `AuthProvider` will see once it has opened the seal.
        let mut header_map = self
            .build_header_map(
                &method,
                path,
                body.as_deref(),
                canonical,
                headers,
                envelope.media_type(),
            )
            .await?;
        // What is bound is what the request carries: the caller's headers
        // and the authorizer's included.
        let bound = bound_headers(&header_map)?;
        let params: Vec<&str> = route.params.iter().map(String::as_str).collect();
        let mut call = SealCall::new(
            method.as_str(),
            RouteRef::new(&route.template, &params),
            contract_sha,
        )
        .query(canonical)
        .payload(body.as_deref().unwrap_or_default(), C::CONTENT_TYPE)
        .accept(self.codec.payload_accept());
        if let Some(key) = bound.idempotency_key.as_deref() {
            call = call.idempotency_key(key);
        }
        if let Some(if_match) = bound.if_match.as_deref() {
            call = call.if_match(if_match);
        }
        let SealedCall {
            body: sealed,
            headers: sealed_headers,
            pending,
        } = envelope.seal_call(call).await?;
        // The seal's own headers win over whatever built them first.
        for (name, value) in sealed_headers {
            header_map.insert(name, value);
        }

        let response = self
            .http
            .request(method.clone(), url.clone())
            .headers(header_map)
            .body(sealed)
            // A replay carries the same `cti`, which the server refuses.
            .with_extension(RequestIdempotency::new(false))
            .send()
            .await?;
        // A client that follows redirects (a caller-supplied one) may have
        // answered from somewhere the request was never sealed for.
        if response.url() != &url {
            return Err(EnvelopeError::Unverified.into());
        }
        let status = response.status();
        let response_headers = response.headers().clone();
        let bytes = response.bytes().await?;
        self.record_request(method.as_str(), path, status, &response_headers)?;

        if !is_cose(&response_headers) {
            // The unsigned "this op's shape is not served": a hint, never
            // proof. Only its body's code says it is not a proxy's own 426.
            if status == StatusCode::UPGRADE_REQUIRED
                && self.is_contract_refusal(&response_headers, &bytes)
            {
                return Err(EnvelopeError::ContractUnsupported {
                    op: refused_op(&method, &route.template),
                }
                .into());
            }
            return Err(EnvelopeError::Unsigned {
                status: status.as_u16(),
            }
            .into());
        }
        let opened = pending
            .open(status.as_u16(), &response_headers, bytes)
            .await?;
        Ok(RuntimeResponseWire {
            status_code: status.as_u16(),
            headers: opened_headers(&response_headers, &opened.payload_type),
            body: opened.body.to_vec(),
        })
    }
}

/// The response's headers as the plain call would have seen them: the body
/// is the opened payload, so its framing headers (and the layer's own
/// `Cratestack-Payload-Type`) go and its type is the one it was sealed under.
/// Everything else (`ETag`, `Retry-After`, `Idempotency-Replayed`) is passed
/// on, and stays unauthenticated.
fn opened_headers(headers: &HeaderMap, payload_type: &str) -> Vec<RuntimeHeader> {
    let dropped = |name: &str| {
        [
            CONTENT_TYPE.as_str(),
            CONTENT_LENGTH.as_str(),
            CONTENT_ENCODING.as_str(),
            TRANSFER_ENCODING.as_str(),
            PAYLOAD_TYPE_HEADER,
        ]
        .iter()
        .any(|dropped| name.eq_ignore_ascii_case(dropped))
    };
    let mut kept: Vec<RuntimeHeader> = headers_to_runtime(headers)
        .into_iter()
        .filter(|header| !dropped(&header.name))
        .collect();
    kept.push(RuntimeHeader {
        name: CONTENT_TYPE.as_str().to_owned(),
        value: payload_type.to_owned(),
    });
    kept
}
