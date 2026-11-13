//! `POST /rpc/batch` on [`RpcClient`]. Split out of `client.rs` for the
//! ~200-LoC ceiling when cratestack#1352 added `batch_response`.

use cratestack_core::rpc::{RPC_BATCH_PATH, RpcRequest, RpcResponseFrame};
use reqwest::Method;

use crate::client::RouteRef;
use crate::codec::HttpClientCodec;
use crate::rpc::client::RpcClient;
use crate::rpc::error::{RpcClientError, client_error_to_rpc, decode_rpc_unary_response};

// `RPC_BATCH_PATH` from core is the axum-template form `"/rpc/batch"`,
// so we just reuse it. The unary path is templated (`/rpc/{op_id}`) so
// we format it per call instead of using the constant directly.
const RPC_BATCH_PATH_PLAIN: &str = RPC_BATCH_PATH;

impl<C> RpcClient<C>
where
    C: HttpClientCodec + Clone,
{
    /// POST /rpc/batch — sequence of `RpcRequest` frames in, sequence of
    /// `RpcResponseFrame` frames out. Per-frame errors do not poison the
    /// batch (each frame's `output` / `error` is reported independently).
    pub async fn batch(
        &self,
        requests: &[RpcRequest],
    ) -> Result<Vec<RpcResponseFrame>, RpcClientError> {
        self.batch_response(requests)
            .await
            .map(|(_, frames)| frames)
    }

    /// [`batch`](Self::batch), also returning the response `Content-Type`,
    /// which `BatchResults::take` needs to re-encode each frame's output in
    /// the format the server answered in (cratestack#1352).
    pub(crate) async fn batch_response(
        &self,
        requests: &[RpcRequest],
    ) -> Result<(String, Vec<RpcResponseFrame>), RpcClientError> {
        let body = self
            .inner
            .codec
            .encode(&requests)
            .map_err(RpcClientError::Codec)?;
        let response = self
            .inner
            .at(RouteRef::rpc("batch"))
            .request_raw_with_query_and_accept(
                Method::POST,
                RPC_BATCH_PATH_PLAIN,
                Some(body),
                None,
                &[],
                None,
            )
            .await
            .map_err(client_error_to_rpc)?;
        let frames =
            decode_rpc_unary_response::<C, Vec<RpcResponseFrame>>(&self.inner.codec, &response)?;
        // Present: `decode_rpc_unary_response` refuses a response without one.
        let content_type = response
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case("content-type"))
            .map(|header| header.value.clone())
            .unwrap_or_default();
        Ok((content_type, frames))
    }
}
