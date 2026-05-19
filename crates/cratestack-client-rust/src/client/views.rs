// `get_view` / `list_view` / `list_view_paged` are projection-view
// reads. The server advertises both `application/cbor` and
// `application/json` on the model read routes that back these
// projections (see `model_read_transport_capabilities_tokens` in
// `cratestack-macros`), so we deliberately do **not** hardcode a
// JSON content type — the request goes out with the client's
// configured Accept (`self.codec.accept_header_value()`) and the
// response is decoded through the same codec into
// `serde_json::Value`, which `CborCodec` and `JsonCodec` both
// support. That keeps the projection-view client surface available
// under either codec configuration; only the `JsonCodec` wrapper
// type itself is gated on `codec-json`.

use cratestack_core::{Page, ProjectionDecoder};
use reqwest::Method;
use serde_json::Value as JsonValue;

use crate::client::core::CratestackClient;
use crate::client::decode::decode_typed_response;
use crate::client::helpers::canonical_query_from_selection;
use crate::codec::HttpClientCodec;
use crate::error::{ClientError, HeaderPair, QueryPair};

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
            .request_raw_with_query(Method::GET, path, None, canonical_query.as_deref(), headers)
            .await?;
        let value: JsonValue = decode_typed_response(&self.codec, &response)?;
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
            .request_raw_with_query(Method::GET, path, None, canonical_query.as_deref(), headers)
            .await?;
        let value: JsonValue = decode_typed_response(&self.codec, &response)?;
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
            .request_raw_with_query(Method::GET, path, None, canonical_query.as_deref(), headers)
            .await?;
        let value: JsonValue = decode_typed_response(&self.codec, &response)?;
        projection.decode_page(value).map_err(ClientError::from)
    }
}
