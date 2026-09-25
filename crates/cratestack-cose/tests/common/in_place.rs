//! Support for the encode-in-place tests: a CBOR codec that records where
//! its output landed, and a value of any encoded size.

use std::sync::{Arc, Mutex};

use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CratestackCodec, CratestackError};
use serde::{Deserialize, Serialize};

/// `CborCodec`, plus: `encode_into` first reserves `reserve` bytes (so the
/// buffer does not grow, and move, while it writes), and records the
/// address and length of what it wrote, as it lies in the buffer after
/// writing.
#[derive(Clone, Default)]
pub struct RecordingCbor {
    pub reserve: usize,
    pub wrote: Arc<Mutex<Option<(usize, usize)>>>,
}

impl CratestackCodec for RecordingCbor {
    const CONTENT_TYPE: &'static str = CborCodec::CONTENT_TYPE;

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CratestackError> {
        CborCodec.encode(value)
    }

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CratestackError> {
        CborCodec.decode(bytes)
    }

    fn encode_into<T: Serialize + ?Sized>(
        &self,
        value: &T,
        out: &mut Vec<u8>,
    ) -> Result<(), CratestackError> {
        out.reserve(self.reserve);
        let start = out.len();
        CborCodec.encode_into(value, out)?;
        let written = &out[start..];
        *self.wrote.lock().expect("lock") = Some((written.as_ptr() as usize, written.len()));
        Ok(())
    }
}

/// A value that `CborCodec` encodes as a text string of `len` bytes, so a
/// test can pick the payload size (and so the payload `bstr` head size).
#[derive(Serialize)]
#[serde(transparent)]
pub struct Text(pub String);

impl Text {
    pub fn of(len: usize) -> Self {
        Self("x".repeat(len))
    }
}
