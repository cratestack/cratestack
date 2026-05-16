use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::CoolCodec;
use reqwest::Method;
use serde_json::Value as JsonValue;

use crate::client::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::error::ClientError;
use crate::runtime::wire::{
    RuntimeErrorWire, RuntimeHeader, RuntimeRequestWire, RuntimeResponseWire,
};
use crate::state::PersistedClientState;
use crate::streaming_callback::{RuntimeChunkWire, pump_streamed_response_callback};

pub(crate) const BRIDGE_CONTENT_TYPE: &str = "application/json";

pub(crate) enum RuntimeTransportClient {
    Cbor(CratestackClient<CborCodec>),
    Json(CratestackClient<JsonCodec>),
}

async fn execute_streamed_transport<C>(
    client: &CratestackClient<C>,
    request: RuntimeRequestWire,
    accept: &'static str,
) -> Result<reqwest::Response, ClientError>
where
    C: HttpClientCodec,
{
    let method = Method::from_bytes(request.method.as_bytes()).map_err(|error| {
        ClientError::BadInput(format!("invalid HTTP method '{}': {error}", request.method))
    })?;
    let header_pairs = request
        .headers
        .iter()
        .map(|header| (header.name.as_str(), header.value.as_str()))
        .collect::<Vec<_>>();
    client
        .request_streamed_with_query_and_accept(
            method,
            &request.path,
            if request.body.is_empty() {
                None
            } else {
                Some(request.body)
            },
            request.canonical_query.as_deref(),
            &header_pairs,
            accept,
        )
        .await
}

fn replace_bridge_content_type(headers: &mut Vec<RuntimeHeader>) {
    headers.retain(|header| !header.name.eq_ignore_ascii_case("content-type"));
    headers.push(RuntimeHeader {
        name: "content-type".to_owned(),
        value: BRIDGE_CONTENT_TYPE.to_owned(),
    });
}

impl RuntimeTransportClient {
    pub(crate) async fn execute_raw(
        &self,
        request: RuntimeRequestWire,
    ) -> Result<RuntimeResponseWire, ClientError> {
        let request = self.bridge_request_to_transport(request)?;
        match self {
            Self::Cbor(client) => client.execute_raw_transport(request).await,
            Self::Json(client) => client.execute_raw_transport(request).await,
        }
        .and_then(|response| self.transport_response_to_bridge(response))
    }

    /// Streaming variant of `execute_raw` for the FFI surface. The
    /// callback receives one [`RuntimeChunkWire`] per complete
    /// cbor-seq item; returning `false` cancels the stream. Returns
    /// when the stream terminates by completion, error, or
    /// cancellation. The success response body is **not buffered** —
    /// items reach the callback as they arrive on the wire.
    pub(crate) async fn execute_streamed<F>(
        &self,
        request: RuntimeRequestWire,
        on_chunk: F,
    ) -> Result<(), RuntimeErrorWire>
    where
        F: FnMut(RuntimeChunkWire) -> bool + Send,
    {
        let request = self
            .bridge_request_to_transport(request)
            .map_err(RuntimeErrorWire::from)?;
        let response = match self {
            Self::Cbor(client) => {
                let accept = client.codec.sequence_accept_header_value();
                execute_streamed_transport(client, request, accept).await
            }
            Self::Json(client) => {
                let accept = client.codec.sequence_accept_header_value();
                execute_streamed_transport(client, request, accept).await
            }
        };
        let response = response.map_err(RuntimeErrorWire::from)?;
        pump_streamed_response_callback(response, on_chunk).await
    }

    fn bridge_request_to_transport(
        &self,
        request: RuntimeRequestWire,
    ) -> Result<RuntimeRequestWire, ClientError> {
        if request.body.is_empty() {
            return Ok(request);
        }

        let value: JsonValue = serde_json::from_slice(&request.body).map_err(|error| {
            ClientError::BadInput(format!("invalid bridge payload JSON: {error}"))
        })?;
        let body = match self {
            Self::Cbor(_) => CborCodec.encode(&value)?,
            Self::Json(_) => JsonCodec.encode(&value)?,
        };

        Ok(RuntimeRequestWire { body, ..request })
    }

    fn transport_response_to_bridge(
        &self,
        mut response: RuntimeResponseWire,
    ) -> Result<RuntimeResponseWire, ClientError> {
        if response.body.is_empty() {
            replace_bridge_content_type(&mut response.headers);
            return Ok(response);
        }

        let value = match self {
            Self::Cbor(_) => CborCodec.decode::<JsonValue>(&response.body)?,
            Self::Json(_) => JsonCodec.decode::<JsonValue>(&response.body)?,
        };

        response.body = serde_json::to_vec(&value).map_err(|error| {
            ClientError::InvalidResponse(format!("failed to encode bridge payload JSON: {error}"))
        })?;
        replace_bridge_content_type(&mut response.headers);
        Ok(response)
    }

    pub(crate) fn state(&self) -> Result<PersistedClientState, ClientError> {
        match self {
            Self::Cbor(client) => client.state(),
            Self::Json(client) => client.state(),
        }
    }
}
