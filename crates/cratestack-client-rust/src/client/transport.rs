use reqwest::Method;

use crate::client::core::CratestackClient;
use crate::client::helpers::{build_url, headers_to_runtime};
use crate::codec::HttpClientCodec;
use crate::error::{ClientError, HeaderPair, QueryPair};
use crate::idempotency::RequestIdempotency;
use crate::runtime::wire::RuntimeResponseWire;

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// The [`RequestIdempotency`] this request goes out with: the
    /// per-client override from
    /// [`CratestackClient::with_idempotency`](crate::CratestackClient::with_idempotency)
    /// when one is set, otherwise the RFC 9110 method-derived default.
    ///
    /// Computed for every request on both transports, plain or
    /// middleware — a `Copy` bool wrapper, so the plain path pays a
    /// `match` and nothing else (`client::http` explains why it cannot
    /// be observed there).
    pub(super) fn request_idempotency(&self, method: &Method) -> RequestIdempotency {
        self.idempotency
            .unwrap_or_else(|| RequestIdempotency::for_method(method))
    }

    pub(crate) async fn request_raw(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        query: &[QueryPair<'_>],
        headers: &[HeaderPair<'_>],
    ) -> Result<RuntimeResponseWire, ClientError> {
        let canonical_query =
            if query.is_empty() {
                None
            } else {
                Some(serde_urlencoded::to_string(query).map_err(|error| {
                    ClientError::BadInput(format!("invalid query pairs: {error}"))
                })?)
            };
        self.request_raw_with_query(method, path, body, canonical_query.as_deref(), headers)
            .await
    }

    pub(crate) async fn request_raw_with_query_and_accept(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        canonical_query: Option<&str>,
        headers: &[HeaderPair<'_>],
        accept_override: Option<&'static str>,
    ) -> Result<RuntimeResponseWire, ClientError> {
        self.send_buffered(
            method,
            path,
            body,
            canonical_query,
            headers,
            accept_override,
            None,
        )
        .await
    }

    pub(super) async fn send_buffered(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        canonical_query: Option<&str>,
        headers: &[HeaderPair<'_>],
        accept_override: Option<&'static str>,
        prepared: Option<&super::prepared::PreparedCall>,
    ) -> Result<RuntimeResponseWire, ClientError> {
        #[cfg(feature = "cose")]
        if let Some(envelope) = &self.sealing.envelope {
            // A sealed call names its own `Accept`: only the seal is asked for.
            return self
                .request_sealed(
                    envelope,
                    method,
                    path,
                    body,
                    canonical_query,
                    headers,
                    prepared,
                )
                .await;
        }
        let url = build_url(&self.config.base_url, path, canonical_query)?;
        let plain_accept = accept_override.unwrap_or_else(|| self.codec.accept_header_value());
        let compact = match prepared {
            Some(call) => Some(self.prepared_compact(call)?),
            None => self.compact_call(&method, headers, body.is_some()),
        };
        let (body, content_type, accept) = if let Some(call) = prepared {
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
        if let Some(call) = prepared {
            call.restore_identity_headers(&mut header_map)?;
            // An authorizer may supply credentials, but cannot change the
            // media types bound to the saved inner request.
            header_map.insert(
                reqwest::header::ACCEPT,
                reqwest::header::HeaderValue::from_str(&accept)
                    .map_err(|error| ClientError::BadInput(error.to_string()))?,
            );
            header_map.insert(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_static(content_type),
            );
        }
        if let Some(call) = &compact {
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
        let headers = response.headers().clone();
        let bytes = response.bytes().await?;
        let mut response_wire = RuntimeResponseWire {
            status_code: status.as_u16(),
            headers: headers_to_runtime(&headers),
            body: bytes.to_vec(),
        };

        self.record_request(method.as_str(), path, status, &headers)?;

        if let Some(call) = &compact {
            response_wire = call.open_response(response_wire)?;
        }
        Ok(response_wire)
    }

    pub(crate) async fn request_raw_with_query(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        canonical_query: Option<&str>,
        headers: &[HeaderPair<'_>],
    ) -> Result<RuntimeResponseWire, ClientError> {
        self.request_raw_with_query_and_accept(method, path, body, canonical_query, headers, None)
            .await
    }
}
