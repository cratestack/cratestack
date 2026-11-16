//! A signed subscription open (ADR 0006, cratestack#1378): the open is
//! sealed like any call, and the stream that answers carries plain frames,
//! labelled `Cratestack-Frames: unsigned`.
//!
//! Only a subscription the schema marks `@frames(unsigned)` is opened this
//! way. The request is `POST` with the input as its CBOR body on RPC (a
//! `GET` cannot carry a body through `fetch`, cratestack#1407) and `GET`
//! with the canonical query on REST. `Accept` names the stream type and
//! `application/cose`: a stream if the server accepts the open, a sealed
//! refusal if it does not.
//!
//! **The unsigned frames are hints.** A response is accepted as the stream
//! only when it is a 2xx that carries the marker exactly once, with the
//! value `unsigned`, in the stream type that was asked for
//! ([`check_unsigned_stream`]); a plain `200` a proxy forged, or one that
//! lost the marker on the way, is refused with
//! [`EnvelopeError::UnsignedFramesRefused`]. Every refusal the server
//! sends is sealed, and is opened here. Every open seals anew, with a fresh
//! nonce, so a resubscribe is never a replay.

use cratestack_core::FRAMES_HEADER;
use reqwest::Method;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_TYPE, HeaderMap};

use super::decode::remote_error_from_response;
use super::envelope_headers::opened_headers;
use super::envelope_refusal::refused_op;
use super::helpers::build_url;
use crate::client::core::CratestackClient;
use crate::client::route::RouteRef;
use crate::codec::HttpClientCodec;
use crate::envelope::{SealCall, is_cose};
use crate::envelope_error::EnvelopeError;
use crate::error::ClientError;
use crate::runtime::wire::RuntimeResponseWire;
use crate::subscription::SubscriptionTarget;

/// The value the marker must have.
const UNSIGNED: &str = "unsigned";

/// Whether a 2xx answer that is not a COSE message is the stream this open
/// asked for: `Cratestack-Frames` appears exactly once with the value
/// `unsigned`, and the `Content-Type` is `stream_type` (parameters and case
/// aside). Anything else is [`EnvelopeError::UnsignedFramesRefused`]: an
/// unmarked body is never read as frames.
pub(crate) fn check_unsigned_stream(
    headers: &HeaderMap,
    stream_type: &str,
) -> Result<(), EnvelopeError> {
    let mut markers = headers.get_all(FRAMES_HEADER).iter();
    let marked = matches!(
        (markers.next(), markers.next()),
        (Some(value), None) if value.as_bytes() == UNSIGNED.as_bytes()
    );
    let mut types = headers.get_all(CONTENT_TYPE).iter();
    let typed = matches!(
        (types.next(), types.next()),
        (Some(value), None) if value.to_str().is_ok_and(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case(stream_type)
        })
    );
    match marked && typed {
        true => Ok(()),
        false => Err(EnvelopeError::UnsignedFramesRefused),
    }
}

/// A sealed answer to a stream open is a refusal: a sealed success is no
/// stream, and is never read as one.
pub(crate) fn reject_sealed_success(status: StatusCode) -> Result<(), ClientError> {
    match status.is_success() {
        true => Err(ClientError::InvalidResponse(
            "a signed subscription open was answered with a sealed success, not a stream"
                .to_owned(),
        )),
        false => Ok(()),
    }
}

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// Seal the open of `target` and send it; the response is the unsigned
    /// stream, `stream_type`, once it has passed [`check_unsigned_stream`].
    /// The caller has checked that `target` is marked.
    pub(crate) async fn open_sealed_stream(
        &self,
        target: &SubscriptionTarget,
        stream_type: &'static str,
    ) -> Result<reqwest::Response, ClientError> {
        let envelope = self
            .sealing
            .envelope
            .as_ref()
            .ok_or(EnvelopeError::StreamsUnsupported)?;
        let path = target.sse_path.as_str();
        // A REST template starts with `/`; an RPC route is
        // `subscribe/<op id>`, whose contract is the op's own.
        let rest = target.seal_route.starts_with('/');
        let contract_key = target
            .seal_route
            .strip_prefix("subscribe/")
            .unwrap_or(&target.seal_route);
        let (method, body, query) = match rest {
            true => (Method::GET, None, target.sse_query.as_deref()),
            false => (Method::POST, Some(self.codec.encode(&target.input)?), None),
        };
        let (contract_sha, _) = self.contract_for(method.as_str(), contract_key)?;
        let url = build_url(&self.config.base_url, path, query)?;
        let header_map = self
            .build_header_map(&method, path, body.as_deref(), query, &[], stream_type)
            .await?;
        let route = match rest {
            true => RouteRef::new(&target.seal_route, &[]),
            false => RouteRef::rpc(&target.seal_route),
        };
        let call = SealCall::new(method.as_str(), route, contract_sha)
            .query(query)
            .payload(body.as_deref().unwrap_or_default(), C::CONTENT_TYPE)
            .accept(self.codec.payload_accept());
        let accept = format!("{stream_type}, {}", envelope.media_type());
        let (response, pending) = self
            .seal_and_send(envelope, &method, url, header_map, call, Some(&accept))
            .await?;
        let status = response.status();
        let headers = response.headers().clone();
        self.record_request(method.as_str(), path, status, &headers)?;

        if is_cose(&headers) {
            let bytes = response.bytes().await?;
            let opened = pending.open(status.as_u16(), &headers, bytes).await?;
            reject_sealed_success(status)?;
            let wire = RuntimeResponseWire {
                status_code: status.as_u16(),
                headers: opened_headers(&headers, &opened.payload_type),
                body: opened.body.to_vec(),
            };
            return Err(remote_error_from_response(&self.codec, &wire));
        }
        if !status.is_success() {
            let bytes = response.bytes().await?;
            // The unsigned "this op's shape is not served": a hint, never
            // proof, as on a unary call.
            if status == StatusCode::UPGRADE_REQUIRED && self.is_contract_refusal(&headers, &bytes)
            {
                return Err(EnvelopeError::ContractUnsupported {
                    op: refused_op(&method, contract_key),
                }
                .into());
            }
            return Err(EnvelopeError::Unsigned {
                status: status.as_u16(),
            }
            .into());
        }
        check_unsigned_stream(&headers, stream_type)?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests;
