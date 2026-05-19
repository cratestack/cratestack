// `get_view` / `list_view` / `list_view_paged` are projection-view
// reads. The server projection routes always respond with JSON
// (the `?select=…` shape isn't currently exposed over CBOR), so
// these methods always negotiate `application/json` and decode the
// body with `serde_json` directly — they do **not** route through
// the client's underlying `HttpClientCodec`. That keeps them
// available regardless of the `codec-json` feature, which only
// gates the `JsonCodec` wrapper type and JSON content-negotiation
// fallback on `CborCodec`.

use cratestack_core::{CoolErrorResponse, Page, ProjectionDecoder};
use reqwest::{Method, StatusCode};
use serde_json::Value as JsonValue;

use crate::client::core::CratestackClient;
use crate::client::helpers::canonical_query_from_selection;
use crate::codec::HttpClientCodec;
use crate::error::{ClientError, HeaderPair, QueryPair};
use crate::runtime::wire::RuntimeResponseWire;

const VIEW_CONTENT_TYPE: &str = "application/json";

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
