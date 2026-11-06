//! One sealed call: seal the request, send it, open the response
//! (ADR 0006, cratestack#1007).
//!
//! The sealing and the opening are [`ClientEnvelope::seal_call`] and
//! [`PendingResponse::open`](crate::PendingResponse::open), the same pair a
//! caller doing its own HTTP uses: this file is the transport around them.
//!
//! With compact wire keys on (`with_compact_keys`), the call is adaptive
//! exactly as a plain one is: it asks for the compact payload types first
//! and seals its request under one only once the op has answered compact.
//! See `compact_wire.rs`; an unsigned `415` turns the op back to named.
//!
//! The result is a [`RuntimeResponseWire`] whose body is the *opened*
//! payload and whose `Content-Type` is the type it was sealed under (CBOR
//! unless the codec says otherwise; a compact answer is turned back into
//! named bytes first), so everything
//! downstream of `request_raw_with_query_and_accept` (typed decoding, the
//! `Remote` error mapping, cbor-seq lists, RPC frames) is the code a plain
//! call runs, with no second path to keep in step with it.

use reqwest::Method;
use reqwest::StatusCode;

use super::envelope_headers::opened_headers;
use crate::client::bound_headers::{bound_headers, refuse_duplicates};
use crate::client::core::CratestackClient;
use crate::client::envelope_refusal::refused_op;
use crate::client::helpers::build_url;
use crate::client::route::RouteRef;
use crate::codec::HttpClientCodec;
use crate::envelope::{ClientEnvelope, SealCall, SealedCall, is_cose};
use crate::envelope_error::EnvelopeError;
use crate::error::{ClientError, HeaderPair};
use crate::idempotency::RequestIdempotency;
use crate::runtime::wire::RuntimeResponseWire;

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
        prepared: Option<&super::prepared::PreparedCall>,
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
        // Compact keys, when opted in: the request payload goes out compact
        // once this op has answered compact, and the payload accept names
        // the compact types first. Neither touches the seal's own framing.
        let compact = match prepared {
            Some(call) => Some(self.prepared_compact(call)?),
            None => self.compact_call(&method, headers, body.is_some()),
        };
        let plain_accept = self.codec.payload_accept();
        let (body, payload_type, payload_accept) = if let Some(call) = prepared {
            if contract_sha != call.contract {
                return Err(super::prepared::bad("prepared sealed contract differs"));
            }
            (body, call.wire_type::<C>()?, call.accept.clone())
        } else {
            match &compact {
                Some(call) => {
                    let sent = call.prepare(body, C::CONTENT_TYPE, plain_accept, false);
                    (sent.body, sent.content_type, sent.accept)
                }
                None => (body, C::CONTENT_TYPE, plain_accept.to_owned()),
            }
        };
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
        .payload(body.as_deref().unwrap_or_default(), payload_type)
        .accept(&payload_accept);
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
            // An unsigned `415` is the server refusing the compact request
            // type: the op goes back to named requests from the next call.
            // This call is not retried.
            if let Some(call) = &compact {
                call.note_failure(status.as_u16());
            }
            return Err(EnvelopeError::Unsigned {
                status: status.as_u16(),
            }
            .into());
        }
        let opened = pending
            .open(status.as_u16(), &response_headers, bytes)
            .await?;
        let wire = RuntimeResponseWire {
            status_code: status.as_u16(),
            headers: opened_headers(&response_headers, &opened.payload_type),
            body: opened.body.to_vec(),
        };
        // A compact answer (sealed under a compact type) is turned back into
        // named bytes, and the op's flag follows what came back.
        match &compact {
            Some(call) => call.open_response(wire),
            None => Ok(wire),
        }
    }
}
