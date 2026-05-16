use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::client::core::CratestackClient;
use crate::client::decode::decode_sequence_response;
use crate::codec::HttpClientCodec;
use crate::error::{ClientError, HeaderPair};
use crate::runtime::wire::{RuntimeRequestWire, RuntimeResponseWire};
use crate::streaming::pump_streamed_response_typed;

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    pub async fn post_list<Input, Output>(
        &self,
        path: &str,
        input: &Input,
        headers: &[HeaderPair<'_>],
    ) -> Result<Vec<Output>, ClientError>
    where
        Input: Serialize,
        Output: DeserializeOwned,
    {
        let body = self.codec.encode(input)?;
        let response = self
            .request_raw_with_query_and_accept(
                Method::POST,
                path,
                Some(body),
                None,
                headers,
                Some(self.codec.sequence_accept_header_value()),
            )
            .await?;
        decode_sequence_response(&self.codec, &response)
    }

    /// Streaming variant of [`Self::post_list`]. Returns an
    /// `mpsc::Receiver` that yields decoded items as they arrive over
    /// the network — first-item latency drops from "buffer the whole
    /// body" to "decode one chunk." Useful on mobile / flaky links
    /// where time-to-first-byte matters more than total throughput.
    ///
    /// The receiver yields `Result<Output, ClientError>` per item.
    /// Transport / decode errors are terminal — the next call to
    /// `.recv()` returns `None` after one. A clean end-of-stream
    /// (server closed cleanly after the last item) also surfaces as
    /// `None` from the next `.recv()`.
    ///
    /// The server must return `application/cbor-seq`. If it returns a
    /// buffered `application/cbor` or `application/json` instead, the
    /// caller should use [`Self::post_list`] — this method does not
    /// fall back.
    pub async fn post_list_streamed<Input, Output>(
        &self,
        path: &str,
        input: &Input,
        headers: &[HeaderPair<'_>],
    ) -> Result<tokio::sync::mpsc::Receiver<Result<Output, ClientError>>, ClientError>
    where
        Input: Serialize,
        Output: DeserializeOwned + Send + 'static,
    {
        let body = self.codec.encode(input)?;
        let response = self
            .request_streamed_with_query_and_accept(
                Method::POST,
                path,
                Some(body),
                None,
                headers,
                self.codec.sequence_accept_header_value(),
            )
            .await?;

        // Bounded channel keeps memory tight on the consumer side —
        // 16 items in flight is plenty for a single subscriber.
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(pump_streamed_response_typed::<Output, ClientError, _>(
            response,
            tx,
            std::convert::identity,
        ));
        Ok(rx)
    }

    pub async fn execute_raw_transport(
        &self,
        request: RuntimeRequestWire,
    ) -> Result<RuntimeResponseWire, ClientError> {
        let method = Method::from_bytes(request.method.as_bytes()).map_err(|error| {
            ClientError::BadInput(format!("invalid HTTP method '{}': {error}", request.method))
        })?;
        let header_pairs = request
            .headers
            .iter()
            .map(|header| (header.name.as_str(), header.value.as_str()))
            .collect::<Vec<_>>();
        self.request_raw_with_query(
            method,
            &request.path,
            if request.body.is_empty() {
                None
            } else {
                Some(request.body)
            },
            request.canonical_query.as_deref(),
            &header_pairs,
        )
        .await
    }
}
