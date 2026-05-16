use reqwest::Method;

use crate::client::core::CratestackClient;
use crate::client::decode::remote_error_from_response;
use crate::client::helpers::{build_url, headers_to_runtime};
use crate::codec::HttpClientCodec;
use crate::error::{ClientError, HeaderPair, QueryPair};
use crate::runtime::wire::RuntimeResponseWire;

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
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
        let url = build_url(&self.config.base_url, path, canonical_query)?;
        let accept = accept_override.unwrap_or_else(|| self.codec.accept_header_value());
        let header_map = self.build_header_map(
            &method,
            path,
            body.as_deref(),
            canonical_query,
            headers,
            accept,
        )?;

        let mut request = self.http.request(method.clone(), url).headers(header_map);
        if let Some(body) = body {
            request = request.body(body);
        }

        let response = request.send().await?;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.bytes().await?;
        let response_wire = RuntimeResponseWire {
            status_code: status.as_u16(),
            headers: headers_to_runtime(&headers),
            body: bytes.to_vec(),
        };

        self.record_request(method.as_str(), path, status, &headers)?;

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

    /// Streaming counterpart to `request_raw_with_query_and_accept`.
    /// Same prep (URL, headers, auth, canonical request), but returns
    /// the raw `reqwest::Response` instead of buffering the body — so
    /// callers can drive `bytes_stream()` themselves.
    ///
    /// Rejects non-2xx responses with `ClientError::Remote` after
    /// buffering the body once, since error bodies are bounded by
    /// `CoolErrorResponse` and small. Only successful responses leave
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
        let url = build_url(&self.config.base_url, path, canonical_query)?;
        let header_map = self.build_header_map(
            &method,
            path,
            body.as_deref(),
            canonical_query,
            headers,
            accept,
        )?;

        let mut request = self.http.request(method.clone(), url).headers(header_map);
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
            let error = remote_error_from_response(&self.codec, &response_wire);
            return Err(error);
        }

        Ok(response)
    }
}
