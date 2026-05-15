//! Pluggable codec + envelope traits used by the transport layer.

use serde::{Deserialize, Serialize};

use crate::context::CoolContext;
use crate::error::CoolError;

pub trait CoolCodec: Clone + Send + Sync + 'static {
    const CONTENT_TYPE: &'static str;

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CoolError>;

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CoolError>;
}

pub trait CoolEnvelope: Clone + Send + Sync + 'static {
    fn request_content_type(&self) -> &'static str;

    fn response_content_type(&self) -> &'static str;

    fn open_request(&self, bytes: &[u8], _ctx: &mut CoolContext) -> Result<Vec<u8>, CoolError>;

    fn seal_response(&self, bytes: &[u8], _ctx: &CoolContext) -> Result<Vec<u8>, CoolError>;
}

/// Pass-through envelope used when transport-layer signing is not
/// required.
#[derive(Debug, Clone, Default)]
pub struct NoEnvelope;

impl CoolEnvelope for NoEnvelope {
    fn request_content_type(&self) -> &'static str {
        "application/octet-stream"
    }

    fn response_content_type(&self) -> &'static str {
        "application/octet-stream"
    }

    fn open_request(&self, bytes: &[u8], _ctx: &mut CoolContext) -> Result<Vec<u8>, CoolError> {
        Ok(bytes.to_vec())
    }

    fn seal_response(&self, bytes: &[u8], _ctx: &CoolContext) -> Result<Vec<u8>, CoolError> {
        Ok(bytes.to_vec())
    }
}
