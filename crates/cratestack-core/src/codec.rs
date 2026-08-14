//! Pluggable codec + envelope traits used by the transport layer.

use serde::{Deserialize, Serialize};

use crate::context::CratestackContext;
use crate::error::CratestackError;

pub trait CratestackCodec: Clone + Send + Sync + 'static {
    const CONTENT_TYPE: &'static str;

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CratestackError>;

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CratestackError>;
}

pub trait CratestackEnvelope: Clone + Send + Sync + 'static {
    fn request_content_type(&self) -> &'static str;

    fn response_content_type(&self) -> &'static str;

    fn open_request(
        &self,
        bytes: &[u8],
        _ctx: &mut CratestackContext,
    ) -> Result<Vec<u8>, CratestackError>;

    fn seal_response(
        &self,
        bytes: &[u8],
        _ctx: &CratestackContext,
    ) -> Result<Vec<u8>, CratestackError>;
}

/// Pass-through envelope used when transport-layer signing is not
/// required.
#[derive(Debug, Clone, Default)]
pub struct NoEnvelope;

impl CratestackEnvelope for NoEnvelope {
    fn request_content_type(&self) -> &'static str {
        "application/octet-stream"
    }

    fn response_content_type(&self) -> &'static str {
        "application/octet-stream"
    }

    fn open_request(
        &self,
        bytes: &[u8],
        _ctx: &mut CratestackContext,
    ) -> Result<Vec<u8>, CratestackError> {
        Ok(bytes.to_vec())
    }

    fn seal_response(
        &self,
        bytes: &[u8],
        _ctx: &CratestackContext,
    ) -> Result<Vec<u8>, CratestackError> {
        Ok(bytes.to_vec())
    }
}
