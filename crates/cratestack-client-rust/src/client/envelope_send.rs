//! The send half of a sealed call, shared by a unary call
//! (`envelope_call.rs`) and a subscription open (`sealed_stream.rs`,
//! cratestack#1378): seal the request, put it on the wire, and hand back an
//! answer that is known to come from the URL it was sealed for.

use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use reqwest::{Method, Url};

use crate::client::core::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::envelope::{ClientEnvelope, PendingResponse, SealCall, SealedCall};
use crate::envelope_error::EnvelopeError;
use crate::error::ClientError;
use crate::idempotency::RequestIdempotency;

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// Seal `call`, send it to `url` with `header_map` (the seal's own
    /// headers win over whatever built it), and return the response with the
    /// [`PendingResponse`] that opens it. `accept` replaces the seal's
    /// `Accept`, for an open that also takes an unsigned stream.
    ///
    /// A response from any URL other than `url` is
    /// [`EnvelopeError::Unverified`]: a client that follows redirects (a
    /// caller-supplied one) may have answered from somewhere the request was
    /// never sealed for.
    pub(crate) async fn seal_and_send(
        &self,
        envelope: &ClientEnvelope,
        method: &Method,
        url: Url,
        mut header_map: HeaderMap,
        call: SealCall<'_>,
        accept: Option<&str>,
    ) -> Result<(reqwest::Response, PendingResponse), ClientError> {
        let SealedCall {
            body: sealed,
            headers: sealed_headers,
            pending,
        } = envelope.seal_call(call).await?;
        // The seal's own headers win over whatever built them first.
        for (name, value) in sealed_headers {
            header_map.insert(name, value);
        }
        if let Some(accept) = accept {
            let value = HeaderValue::from_str(accept)
                .map_err(|error| ClientError::BadInput(format!("invalid Accept: {error}")))?;
            header_map.insert(ACCEPT, value);
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
        if response.url() != &url {
            return Err(EnvelopeError::Unverified.into());
        }
        Ok((response, pending))
    }
}
