//! One sealed call: seal the request, send it, open the response
//! (ADR 0006, cratestack#1007).
//!
//! The result is a [`RuntimeResponseWire`] whose body is the *opened*
//! payload and whose `Content-Type` is `application/cbor`, so everything
//! downstream of `request_raw_with_query_and_accept` (typed decoding, the
//! `Remote` error mapping, cbor-seq lists, RPC frames) is the code a plain
//! call runs, with no second path to keep in step with it.

use cratestack_core::{
    Binding, CONTRACT_HEADER, CratestackError, ResponseBinding, canonical_query,
};
use cratestack_cose::request_digest;
use reqwest::Method;
use reqwest::StatusCode;
use reqwest::header::{
    CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue,
    TRANSFER_ENCODING,
};

use crate::client::bound_headers::{bound_headers, refuse_duplicates};
use crate::client::core::CratestackClient;
use crate::client::envelope_refusal::refused_op;
use crate::client::helpers::{build_url, headers_to_runtime};
use crate::codec::HttpClientCodec;
use crate::envelope::ClientEnvelope;
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
        let (contract_sha, selector) = self.contract_for(method.as_str(), &route.template)?;
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
        header_map.insert(
            HeaderName::from_static("cratestack-contract"),
            HeaderValue::from_str(&selector).map_err(|error| {
                ClientError::BadInput(format!("invalid {CONTRACT_HEADER} value: {error}"))
            })?,
        );
        let bound = bound_headers(&header_map)?;
        let query = Some(canonical_query(canonical)).filter(|query| !query.is_empty());
        let binding = envelope.binding(
            method.as_str(),
            &route.template,
            &route.params,
            query,
            contract_sha,
            bound,
        );
        let sealed = envelope
            .cose()
            .seal_request(body.as_deref().unwrap_or_default(), &binding)
            .await
            .map_err(|error| ClientError::from(EnvelopeError::Seal(error)))?;
        header_map.insert(
            CONTENT_TYPE,
            HeaderValue::from_static(envelope.media_type()),
        );

        // The response binds the digest of exactly these bytes, so take it
        // before the body is handed to the transport.
        let sealed_digest = request_digest(&sealed);
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
        let answer = Binding {
            response: Some(ResponseBinding {
                request: sealed_digest,
                status: status.as_u16(),
            }),
            ..binding
        };
        let opened = envelope
            .cose()
            .open_response(bytes, &answer)
            .await
            .map_err(open_error)?;
        Ok(RuntimeResponseWire {
            status_code: status.as_u16(),
            headers: opened_headers(&response_headers),
            body: opened.payload.to_vec(),
        })
    }
}

/// Whether the response names exactly one content type, an
/// `application/cose` one.
fn is_cose(headers: &HeaderMap) -> bool {
    let mut values = headers.get_all(CONTENT_TYPE).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return false;
    };
    value.to_str().is_ok_and(|value| {
        value
            .split(';')
            .next()
            .unwrap_or(value)
            .trim()
            .eq_ignore_ascii_case("application/cose")
    })
}

/// The response's headers as the plain call would have seen them: the body
/// is the opened CBOR payload, so its framing headers go and its type is
/// `application/cbor`. Everything else (`ETag`, `Retry-After`,
/// `Idempotency-Replayed`) is passed on, and stays unauthenticated.
fn opened_headers(headers: &HeaderMap) -> Vec<RuntimeHeader> {
    let mut kept: Vec<RuntimeHeader> = headers_to_runtime(headers)
        .into_iter()
        .filter(|header| {
            ![
                CONTENT_TYPE,
                CONTENT_LENGTH,
                CONTENT_ENCODING,
                TRANSFER_ENCODING,
            ]
            .iter()
            .any(|name| header.name.eq_ignore_ascii_case(name.as_str()))
        })
        .collect();
    kept.push(RuntimeHeader {
        name: CONTENT_TYPE.as_str().to_owned(),
        value: "application/cbor".to_owned(),
    });
    kept
}

/// Verification failures are the coarse `401` (ADR 0006 §10); anything else
/// is a backend, not the message.
fn open_error(error: CratestackError) -> ClientError {
    match error {
        CratestackError::Unauthorized(_) => EnvelopeError::Unverified,
        other => EnvelopeError::Open(other),
    }
    .into()
}
