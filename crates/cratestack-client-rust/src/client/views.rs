use cratestack_codec_json::JsonCodec;
use cratestack_core::{CoolCodec, Page, ProjectionDecoder};
use reqwest::Method;

use crate::client::core::CratestackClient;
use crate::client::decode::decode_json_value_response;
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
            .request_raw_with_query_and_accept(
                Method::GET,
                path,
                None,
                canonical_query.as_deref(),
                headers,
                Some(JsonCodec::CONTENT_TYPE),
            )
            .await?;
        let value = decode_json_value_response(&JsonCodec, &response)?;
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
                Some(JsonCodec::CONTENT_TYPE),
            )
            .await?;
        let value = decode_json_value_response(&JsonCodec, &response)?;
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
                Some(JsonCodec::CONTENT_TYPE),
            )
            .await?;
        let value = decode_json_value_response(&JsonCodec, &response)?;
        projection.decode_page(value).map_err(ClientError::from)
    }
}
