use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::client::core::CratestackClient;
use crate::client::decode::decode_typed_response;
use crate::codec::HttpClientCodec;
use crate::error::{ClientError, HeaderPair, QueryPair};

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    pub async fn get<Output>(
        &self,
        path: &str,
        query: &[QueryPair<'_>],
        headers: &[HeaderPair<'_>],
    ) -> Result<Output, ClientError>
    where
        Output: DeserializeOwned,
    {
        let response = self
            .request_raw(Method::GET, path, None, query, headers)
            .await?;
        decode_typed_response(&self.codec, &response)
    }

    pub async fn post<Input, Output>(
        &self,
        path: &str,
        input: &Input,
        headers: &[HeaderPair<'_>],
    ) -> Result<Output, ClientError>
    where
        Input: Serialize,
        Output: DeserializeOwned,
    {
        let body = self.codec.encode(input)?;
        let response = self
            .request_raw(Method::POST, path, Some(body), &[], headers)
            .await?;
        decode_typed_response(&self.codec, &response)
    }

    pub async fn patch<Input, Output>(
        &self,
        path: &str,
        input: &Input,
        headers: &[HeaderPair<'_>],
    ) -> Result<Output, ClientError>
    where
        Input: Serialize,
        Output: DeserializeOwned,
    {
        let body = self.codec.encode(input)?;
        let response = self
            .request_raw(Method::PATCH, path, Some(body), &[], headers)
            .await?;
        decode_typed_response(&self.codec, &response)
    }

    pub async fn delete<Output>(
        &self,
        path: &str,
        headers: &[HeaderPair<'_>],
    ) -> Result<Output, ClientError>
    where
        Output: DeserializeOwned,
    {
        let response = self
            .request_raw(Method::DELETE, path, None, &[], headers)
            .await?;
        decode_typed_response(&self.codec, &response)
    }
}
