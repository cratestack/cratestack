// `get_view` / `list_view` / `list_view_paged` are projection-view
// reads. The server projection routes always respond with JSON
// (the `?select=…` shape isn't currently exposed over CBOR), so
// these methods always negotiate `application/json` and decode the
// body with `serde_json` directly — they do **not** route through
// the client's underlying `HttpClientCodec`.
//
// The method *signatures* are unconditional so that
// `include_client_schema!`-generated `<Model>Client` types — which
// always emit `list_view`/`get_view` thin wrappers — compile under
// either feature configuration. The method *bodies* are gated on
// `codec-json`: when the feature is off the methods short-circuit
// with `ClientError::BadInput`, so a CBOR-only deployment cannot
// quietly negotiate JSON via these paths.

use cratestack_core::{Page, ProjectionDecoder};

use crate::client::core::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::error::{ClientError, HeaderPair, QueryPair};

#[cfg(feature = "codec-json")]
use cratestack_core::CoolErrorResponse;
#[cfg(feature = "codec-json")]
use reqwest::{Method, StatusCode};
#[cfg(feature = "codec-json")]
use serde_json::Value as JsonValue;

#[cfg(feature = "codec-json")]
use crate::client::helpers::canonical_query_from_selection;
#[cfg(feature = "codec-json")]
use crate::runtime::wire::RuntimeResponseWire;

#[cfg(feature = "codec-json")]
const VIEW_CONTENT_TYPE: &str = "application/json";

#[cfg(feature = "codec-json")]
fn decode_view_json_response(response: &RuntimeResponseWire) -> Result<JsonValue, ClientError> {
    if (200..=299).contains(&response.status_code) {
        serde_json::from_slice(&response.body).map_err(|error| {
            ClientError::InvalidResponse(format!("failed to decode view response as JSON: {error}"))
        })
    } else {
        let error: Option<CoolErrorResponse> = serde_json::from_slice(&response.body).ok();
        let message = error
            .as_ref()
            .map(|value| value.message.clone())
            .unwrap_or_else(|| format!("unexpected error body for status {}", response.status_code));
        Err(ClientError::Remote {
            status: StatusCode::from_u16(response.status_code)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            error,
            message,
        })
    }
}

#[cfg(not(feature = "codec-json"))]
fn view_methods_disabled() -> ClientError {
    ClientError::BadInput(
        "projection-view methods (get_view / list_view / list_view_paged) require \
         cratestack-client-rust to be built with the `codec-json` feature; the server \
         projection routes are JSON-only today"
            .to_owned(),
    )
}

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    pub async fn get_view<P>(
        &self,
        path: &str,
        projection: &P,
        headers: &[HeaderPair<'_>],
    ) -> Result<P::Output, ClientError>
    where
        P: ProjectionDecoder,
    {
        #[cfg(not(feature = "codec-json"))]
        {
            let _ = (path, projection, headers);
            return Err(view_methods_disabled());
        }
        #[cfg(feature = "codec-json")]
        {
            let selection = projection.selection_query();
            let canonical_query = canonical_query_from_selection(&selection, &[])?;
            let response = self
                .request_raw_with_query_and_accept(
                    Method::GET,
                    path,
                    None,
                    canonical_query.as_deref(),
                    headers,
                    Some(VIEW_CONTENT_TYPE),
                )
                .await?;
            let value = decode_view_json_response(&response)?;
            projection.decode_one(value).map_err(ClientError::from)
        }
    }

    pub async fn list_view<P>(
        &self,
        path: &str,
        projection: &P,
        extra_query: &[QueryPair<'_>],
        headers: &[HeaderPair<'_>],
    ) -> Result<Vec<P::Output>, ClientError>
    where
        P: ProjectionDecoder,
    {
        #[cfg(not(feature = "codec-json"))]
        {
            let _ = (path, projection, extra_query, headers);
            return Err(view_methods_disabled());
        }
        #[cfg(feature = "codec-json")]
        {
            let selection = projection.selection_query();
            let canonical_query = canonical_query_from_selection(&selection, extra_query)?;
            let response = self
                .request_raw_with_query_and_accept(
                    Method::GET,
                    path,
                    None,
                    canonical_query.as_deref(),
                    headers,
                    Some(VIEW_CONTENT_TYPE),
                )
                .await?;
            let value = decode_view_json_response(&response)?;
            projection.decode_many(value).map_err(ClientError::from)
        }
    }

    pub async fn list_view_paged<P>(
        &self,
        path: &str,
        projection: &P,
        extra_query: &[QueryPair<'_>],
        headers: &[HeaderPair<'_>],
    ) -> Result<Page<P::Output>, ClientError>
    where
        P: ProjectionDecoder,
    {
        #[cfg(not(feature = "codec-json"))]
        {
            let _ = (path, projection, extra_query, headers);
            return Err(view_methods_disabled());
        }
        #[cfg(feature = "codec-json")]
        {
            let selection = projection.selection_query();
            let canonical_query = canonical_query_from_selection(&selection, extra_query)?;
            let response = self
                .request_raw_with_query_and_accept(
                    Method::GET,
                    path,
                    None,
                    canonical_query.as_deref(),
                    headers,
                    Some(VIEW_CONTENT_TYPE),
                )
                .await?;
            let value = decode_view_json_response(&response)?;
            projection.decode_page(value).map_err(ClientError::from)
        }
    }
}
