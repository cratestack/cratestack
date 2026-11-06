//! The streaming half of the transport: the same request prep as the
//! buffered call, but the successful response is handed back unbuffered.

use reqwest::Method;

use crate::client::compact_wire::ItemTranscoder;
use crate::client::core::CratestackClient;
use crate::client::decode::remote_error_from_response;
use crate::client::helpers::{build_url, headers_to_runtime};
use crate::codec::HttpClientCodec;
use crate::error::{ClientError, HeaderPair};
use crate::runtime::wire::RuntimeResponseWire;

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// Streaming counterpart to `request_raw_with_query_and_accept`.
    /// Same prep (URL, headers, auth, canonical request), but returns
    /// the raw `reqwest::Response` instead of buffering the body — so
    /// callers can drive `bytes_stream()` themselves.
    ///
    /// Rejects non-2xx responses with `ClientError::Remote` after
    /// buffering the body once, since error bodies are bounded by
    /// `CratestackErrorResponse` and small. Only successful responses leave
    /// this method unbuffered.
    pub(crate) async fn request_streamed_with_query_and_accept(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        canonical_query: Option<&str>,
        headers: &[HeaderPair<'_>],
        accept: &'static str,
    ) -> Result<reqwest::Response, ClientError> {
        self.request_streamed_compact(method, path, body, canonical_query, headers, accept, false)
            .await
            .map(|(response, _)| response)
    }

    /// [`Self::request_streamed_with_query_and_accept`], optionally offering
    /// compact keys (`compact`). Alongside the response comes the item
    /// rewrite the pump must apply when the server answered with a compact
    /// sequence.
    pub(crate) async fn request_streamed_compact(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        canonical_query: Option<&str>,
        headers: &[HeaderPair<'_>],
        accept: &'static str,
        compact: bool,
    ) -> Result<(reqwest::Response, Option<ItemTranscoder>), ClientError> {
        // Every stream and subscription goes through here. Sealed streams are
        // ADR 0006 P1; until then a client with an envelope refuses them
        // instead of sending a plain request.
        #[cfg(feature = "cose")]
        if self.sealing.envelope.is_some() {
            return Err(crate::envelope_error::EnvelopeError::StreamsUnsupported.into());
        }
        if compact && self.compact.is_some() {
            super::compact_identity::refuse_duplicates(headers)?;
        }
        let url = build_url(&self.config.base_url, path, canonical_query)?;
        let plan = compact
            .then(|| self.compact_call(&method, headers, body.is_some()))
            .flatten();
        let (body, content_type, accept) = match &plan {
            Some(call) => {
                let sent = call.prepare(body, C::CONTENT_TYPE, accept, true);
                (sent.body, sent.content_type, sent.accept)
            }
            None => (body, C::CONTENT_TYPE, accept.to_owned()),
        };
        let mut header_map = self
            .build_header_map_as(
                &method,
                path,
                body.as_deref(),
                canonical_query,
                headers,
                &accept,
                content_type,
            )
            .await?;
        super::compact_identity::check(plan.is_some(), false, headers, &header_map)?;
        if let Some(call) = &plan {
            call.stamp_selector(&mut header_map)?;
        }

        let mut request = self
            .http
            .request(method.clone(), url)
            .headers(header_map)
            .with_extension(self.request_idempotency(&method));
        if let Some(body) = body {
            request = request.body(body);
        }

        let response = request.send().await?;
        let status = response.status();
        let headers_snapshot = response.headers().clone();
        self.record_request(method.as_str(), path, status, &headers_snapshot)?;

        if !status.is_success() {
            // Bounded error path — buffer the body (small by contract)
            // and produce a Remote error, matching the buffered code
            // path's behavior.
            let bytes = response.bytes().await?;
            let response_wire = RuntimeResponseWire {
                status_code: status.as_u16(),
                headers: headers_to_runtime(&headers_snapshot),
                body: bytes.to_vec(),
            };
            if let Some(call) = &plan {
                call.note_failure(response_wire.status_code);
            }
            let error = remote_error_from_response(&self.codec, &response_wire);
            return Err(error);
        }

        let items = plan.and_then(|call| call.note_stream(&headers_snapshot));
        Ok((response, items))
    }
}
