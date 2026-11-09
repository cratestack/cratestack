use cratestack_core::{CratestackCodec, CratestackError};
use serde::{Deserialize, Serialize};

use crate::{CborCodec, check_depth, decode_error};

/// [`CborCodec`] with a caller-chosen decode depth limit; build it with
/// [`CborCodec::with_max_depth`]. Encoding is identical to `CborCodec`.
#[derive(Debug, Clone, Copy)]
pub struct CborCodecWithDepth {
    max_depth: usize,
}

impl CborCodecWithDepth {
    pub fn new(max_depth: usize) -> Self {
        Self { max_depth }
    }

    pub fn max_depth(&self) -> usize {
        self.max_depth
    }
}

impl CratestackCodec for CborCodecWithDepth {
    const CONTENT_TYPE: &'static str = CborCodec::CONTENT_TYPE;

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CratestackError> {
        CborCodec.encode(value)
    }

    fn encode_into<T: Serialize + ?Sized>(
        &self,
        value: &T,
        out: &mut Vec<u8>,
    ) -> Result<(), CratestackError> {
        CborCodec.encode_into(value, out)
    }

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CratestackError> {
        decode_with_max_depth(bytes, self.max_depth)
    }
}

/// `CborCodec::decode` with an explicit depth limit: the nesting is checked
/// iteratively first (cratestack#1315), then serde decodes the whole of
/// `bytes`, which must be exactly one item.
pub fn decode_with_max_depth<T: for<'de> Deserialize<'de>>(
    bytes: &[u8],
    max_depth: usize,
) -> Result<T, CratestackError> {
    check_depth(bytes, max_depth)?;
    let mut deserializer = minicbor_serde::Deserializer::new(bytes);
    let value = serde_path_to_error::deserialize(&mut deserializer).map_err(decode_error)?;
    let consumed = deserializer.decoder().position();
    if consumed != bytes.len() {
        let left = bytes.len() - consumed;
        let unit = if left == 1 { "byte" } else { "bytes" };
        return Err(decode_error(format!(
            "{left} {unit} left unconsumed after the value"
        )));
    }
    Ok(value)
}
