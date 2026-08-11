use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::client::TypedResponse;
use crate::client::core::CratestackClient;
use crate::client::decode::{decode_typed_response, decode_typed_response_with_metadata};
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

    /// Same request as [`Self::get`], but returns the status and
    /// headers alongside the decoded body (issue #493) — the way a
    /// caller reads the `ETag` off an `@version` model's `GET` before
    /// sending it back as `If-Match` on `patch_with_response`/`patch`.
    pub async fn get_with_response<Output>(
        &self,
        path: &str,
        query: &[QueryPair<'_>],
        headers: &[HeaderPair<'_>],
    ) -> Result<TypedResponse<Output>, ClientError>
    where
        Output: DeserializeOwned,
    {
        let response = self
            .request_raw(Method::GET, path, None, query, headers)
            .await?;
        decode_typed_response_with_metadata(&self.codec, &response)
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

    /// Same request as [`Self::post`], but returns the status and
    /// headers alongside the decoded body (issue #493) — e.g. to read
    /// `Idempotency-Replayed` off a create sent through an
    /// idempotency-key-aware server.
    pub async fn post_with_response<Input, Output>(
        &self,
        path: &str,
        input: &Input,
        headers: &[HeaderPair<'_>],
    ) -> Result<TypedResponse<Output>, ClientError>
    where
        Input: Serialize,
        Output: DeserializeOwned,
    {
        let body = self.codec.encode(input)?;
        let response = self
            .request_raw(Method::POST, path, Some(body), &[], headers)
            .await?;
        decode_typed_response_with_metadata(&self.codec, &response)
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

    /// Same request as [`Self::patch`], but returns the status and
    /// headers alongside the decoded body (issue #493) — the response
    /// carries the *new* `ETag` after a successful `@version` update,
    /// which a caller chaining further updates needs to read back out.
    pub async fn patch_with_response<Input, Output>(
        &self,
        path: &str,
        input: &Input,
        headers: &[HeaderPair<'_>],
    ) -> Result<TypedResponse<Output>, ClientError>
    where
        Input: Serialize,
        Output: DeserializeOwned,
    {
        let body = self.codec.encode(input)?;
        let response = self
            .request_raw(Method::PATCH, path, Some(body), &[], headers)
            .await?;
        decode_typed_response_with_metadata(&self.codec, &response)
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

    /// Same request as [`Self::delete`], but returns the status and
    /// headers alongside the decoded body (issue #493) — `DELETE` on
    /// an `@version` model also requires `If-Match`, so the same
    /// `GET` → `ETag` round trip applies here as it does to `PATCH`.
    pub async fn delete_with_response<Output>(
        &self,
        path: &str,
        headers: &[HeaderPair<'_>],
    ) -> Result<TypedResponse<Output>, ClientError>
    where
        Output: DeserializeOwned,
    {
        let response = self
            .request_raw(Method::DELETE, path, None, &[], headers)
            .await?;
        decode_typed_response_with_metadata(&self.codec, &response)
    }
}
