use chrono::Utc;
use cratestack_core::canonical_request_string;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, StatusCode};

use crate::auth::AuthorizationRequest;
use crate::client::core::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::error::{ClientError, HeaderPair};
use crate::state::RequestJournalEntry;

/// Same header name `cratestack-axum::schema_fingerprint::SCHEMA_SHA_HEADER`
/// checks server-side (issue #178) — duplicated, not shared via a
/// dependency, since this client crate has no reason to depend on the
/// server-runtime crate for one string constant.
const SCHEMA_SHA_HEADER: HeaderName = HeaderName::from_static("x-cratestack-schema-sha");

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// Build a reqwest `HeaderMap` for a request, applying ACCEPT,
    /// CONTENT_TYPE, authorizer-emitted headers, and per-call extras.
    /// Returns the resolved content type (so the journal entry can
    /// record it consistently). Async (issue #453) because
    /// `RequestAuthorizer::authorize` is — every caller is already inside
    /// an async fn (`request_raw_with_query_and_accept` and friends), so
    /// awaiting here is a localized change, not a structural one.
    pub(crate) async fn build_header_map(
        &self,
        method: &Method,
        path: &str,
        body: Option<&[u8]>,
        canonical_query: Option<&str>,
        headers: &[HeaderPair<'_>],
        accept: &'static str,
    ) -> Result<HeaderMap, ClientError> {
        let mut header_map = HeaderMap::new();
        if let Some(schema_sha) = self.schema_sha {
            // Issue #178: warn-only drift detection, never rejected by the
            // server — `HeaderValue::from_static` is safe here because
            // `schema_sha` is always a hex-digest string baked in at
            // macro-expansion time, never user input.
            header_map.insert(SCHEMA_SHA_HEADER, HeaderValue::from_static(schema_sha));
        }
        header_map.insert(ACCEPT, HeaderValue::from_static(accept));
        let content_type = if body.is_some() {
            header_map.insert(CONTENT_TYPE, HeaderValue::from_static(C::CONTENT_TYPE));
            Some(C::CONTENT_TYPE.to_owned())
        } else {
            None
        };
        if let Some(authorizer) = &self.request_authorizer {
            let canonical_request = canonical_request_string(
                method.as_str(),
                path,
                canonical_query,
                content_type.as_deref(),
                body.unwrap_or(&[]),
            );
            let authorization_request = AuthorizationRequest {
                method: method.as_str().to_owned(),
                path: path.to_owned(),
                canonical_query: canonical_query.map(str::to_owned),
                content_type: content_type.clone(),
                body: body.map(<[u8]>::to_vec).unwrap_or_default(),
                canonical_request,
            };
            for (name, value) in authorizer.authorize(&authorization_request).await? {
                header_map.insert(
                    HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                        ClientError::BadInput(format!("invalid header name '{name}': {error}"))
                    })?,
                    HeaderValue::from_str(&value).map_err(|error| {
                        ClientError::BadInput(format!("invalid header value for '{name}': {error}"))
                    })?,
                );
            }
        }
        for (name, value) in headers {
            header_map.insert(
                HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                    ClientError::BadInput(format!("invalid header name '{name}': {error}"))
                })?,
                HeaderValue::from_str(value).map_err(|error| {
                    ClientError::BadInput(format!("invalid header value for '{name}': {error}"))
                })?,
            );
        }
        Ok(header_map)
    }

    pub(crate) fn record_request(
        &self,
        method: &str,
        path: &str,
        status: StatusCode,
        headers: &HeaderMap,
    ) -> Result<(), ClientError> {
        self.state_store
            .append_request_journal(&RequestJournalEntry {
                method: method.to_owned(),
                path: path.to_owned(),
                status_code: status.as_u16(),
                content_type: headers
                    .get(CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(ToOwned::to_owned),
                recorded_at: Utc::now(),
            })
    }
}
