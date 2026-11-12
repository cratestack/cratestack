use std::collections::HashMap;

use cratestack_codec_cbor::CborCodec;
#[cfg(feature = "codec-json")]
use cratestack_codec_json::JsonCodec;
use cratestack_core::rpc::{RpcFrameValue, RpcResponseFrame};
use cratestack_core::{CratestackCodec, CratestackError};

use crate::codec::{HttpClientCodec, media_type_matches};
use crate::remote_refusal::Challenges;
use crate::rpc::batch_call::BatchHandle;
use crate::rpc::error::{RpcClientError, RpcRemoteError, http_status_for_rpc_code};

/// Per-handle results from a sent batch. Each handle can be `take`n
/// exactly once.
///
/// Holds the client's codec and the response `Content-Type` so each frame's
/// output can be decoded the way a unary response would be: re-encoded in
/// the format the server answered in, then read by the codec's
/// `decode_response`. That keeps `Uuid` / `Bytes` outputs in their CBOR
/// forms (cratestack#1352).
pub struct BatchResults<C = CborCodec> {
    codec: C,
    content_type: String,
    frames: HashMap<u64, RpcResponseFrame>,
    encode_errors: HashMap<u64, CratestackError>,
}

impl<C> BatchResults<C> {
    pub(crate) fn new(
        codec: C,
        content_type: String,
        frames: HashMap<u64, RpcResponseFrame>,
        encode_errors: HashMap<u64, CratestackError>,
    ) -> Self {
        Self {
            codec,
            content_type,
            frames,
            encode_errors,
        }
    }
}

impl<C: HttpClientCodec> BatchResults<C> {
    /// Extract the typed output for one queued op. Returns:
    ///
    /// - `Ok(output)` — the server emitted an `output` for this frame
    ///   and it decoded into `O`.
    /// - `Err(RpcClientError::Codec(_))` — the input failed to encode
    ///   before send, or the output failed to decode.
    /// - `Err(RpcClientError::Remote(RpcRemoteError { body, .. }))` —
    ///   the server emitted an `error` frame for this op. The
    ///   `status` field is derived from the gRPC-style code in the
    ///   body since `/rpc/batch` returns 200 at the HTTP level
    ///   regardless of per-frame outcomes.
    /// - `Err(RpcClientError::InvalidResponse(_))` — the server
    ///   omitted this frame entirely or the frame had neither
    ///   `output` nor `error` set.
    pub fn take<O>(&mut self, handle: BatchHandle<O>) -> Result<O, RpcClientError>
    where
        O: serde::de::DeserializeOwned,
    {
        if let Some(error) = self.encode_errors.remove(&handle.id) {
            return Err(RpcClientError::Codec(error));
        }
        let frame = self.frames.remove(&handle.id).ok_or_else(|| {
            RpcClientError::InvalidResponse(format!(
                "batch response missing frame for id {}",
                handle.id,
            ))
        })?;
        match (frame.output, frame.error) {
            (Some(output), None) => self.decode_output(&output).map_err(|error| {
                RpcClientError::Codec(CratestackError::Codec(format!(
                    "decode batch output for id {}: {error}",
                    handle.id,
                )))
            }),
            (None, Some(body)) => Err(RpcClientError::Remote(RpcRemoteError {
                status: http_status_for_rpc_code(&body.code),
                body,
                challenges: Challenges::default(),
            })),
            (Some(_), Some(_)) | (None, None) => Err(RpcClientError::InvalidResponse(format!(
                "batch frame {} has both `output` and `error` set, or neither",
                handle.id,
            ))),
        }
    }

    fn decode_output<O: serde::de::DeserializeOwned>(
        &self,
        output: &RpcFrameValue,
    ) -> Result<O, CratestackError> {
        let content_type = self.content_type.as_str();
        let bytes = if media_type_matches(content_type, C::CONTENT_TYPE) {
            self.codec.encode(output)?
        } else if media_type_matches(content_type, CborCodec::CONTENT_TYPE) {
            CborCodec.encode(output)?
        } else {
            #[cfg(feature = "codec-json")]
            if media_type_matches(content_type, JsonCodec::CONTENT_TYPE) {
                return self
                    .codec
                    .decode_response(content_type, &JsonCodec.encode(output)?);
            }
            return Err(CratestackError::Codec(format!(
                "unsupported response Content-Type {content_type}"
            )));
        };
        self.codec.decode_response(content_type, &bytes)
    }
}
