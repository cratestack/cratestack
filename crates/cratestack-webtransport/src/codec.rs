//! Frames over a byte stream (design §6a.3).
//!
//! A QUIC stream has no message boundaries, so every frame delimits itself:
//! CBOR frames are written back to back as a CBOR sequence (RFC 8742) and
//! split with the same scanner the HTTP streaming path uses; JSON frames are
//! one document per line (NDJSON) — a JSON encoder escapes every newline
//! inside a string, so a raw `\n` only ever ends a frame. Values are encoded
//! with the same `CborCodec` / `JsonCodec` as every other binding, so a frame
//! here is byte-for-byte what those codecs produce elsewhere.

use cratestack_codec_cbor::{CborCodec, CborSeqChunkDecoder};
use cratestack_codec_json::JsonCodec;
use cratestack_core::{CratestackCodec, CratestackError};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// WebTransport subprotocol for CBOR frames (the WS subprotocol name, §6.2).
pub const CBOR_PROTOCOL: &str = "cratestack.rpc.v1+cbor";
/// WebTransport subprotocol for JSON frames.
pub const JSON_PROTOCOL: &str = "cratestack.rpc.v1+json";

/// The codec one session speaks, chosen at CONNECT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireCodec {
    Cbor,
    Json,
}

impl WireCodec {
    pub fn protocol(self) -> &'static str {
        match self {
            Self::Cbor => CBOR_PROTOCOL,
            Self::Json => JSON_PROTOCOL,
        }
    }

    /// Picks the codec for a session: the first offered subprotocol this
    /// server speaks, else the `codec` query parameter (for clients whose
    /// WebTransport stack cannot negotiate subprotocols), else CBOR.
    /// `negotiated` is `true` when the choice came from a subprotocol, so
    /// the server must echo it in the CONNECT response.
    pub fn select(
        offered: &[String],
        query: Option<&str>,
    ) -> Result<(Self, bool), CratestackError> {
        for protocol in offered {
            match protocol.as_str() {
                CBOR_PROTOCOL => return Ok((Self::Cbor, true)),
                JSON_PROTOCOL => return Ok((Self::Json, true)),
                _ => {}
            }
        }
        let from_query = query
            .into_iter()
            .flat_map(|q| q.split('&'))
            .find_map(|pair| pair.strip_prefix("codec="));
        match from_query {
            None | Some("cbor") => Ok((Self::Cbor, false)),
            Some("json") => Ok((Self::Json, false)),
            Some(other) => Err(CratestackError::NotAcceptable(format!(
                "unsupported WebTransport codec `{other}`; expected `cbor` or `json`",
            ))),
        }
    }

    /// One frame's bytes, delimiter included.
    pub fn encode<T: Serialize>(self, frame: &T) -> Result<Vec<u8>, CratestackError> {
        match self {
            Self::Cbor => CborCodec.encode(frame),
            Self::Json => {
                let mut bytes = JsonCodec.encode(frame)?;
                bytes.push(b'\n');
                Ok(bytes)
            }
        }
    }

    pub fn decode<T: DeserializeOwned>(self, frame: &[u8]) -> Result<T, CratestackError> {
        match self {
            Self::Cbor => CborCodec.decode(frame),
            Self::Json => JsonCodec.decode(frame),
        }
    }
}

/// Splits an incoming byte stream into frames, refusing any frame that
/// grows past `max_frame_bytes` before it is complete — so a client cannot
/// make the server buffer an unbounded "frame".
pub struct FrameDecoder {
    codec: WireCodec,
    max_frame_bytes: usize,
    cbor: CborSeqChunkDecoder,
    json: Vec<u8>,
}

impl FrameDecoder {
    pub fn new(codec: WireCodec, max_frame_bytes: usize) -> Self {
        Self {
            codec,
            max_frame_bytes,
            cbor: CborSeqChunkDecoder::new(),
            json: Vec::new(),
        }
    }

    /// Feed the next chunk; returns every frame it completed, in order.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, CratestackError> {
        let frames = match self.codec {
            WireCodec::Cbor => self.cbor.feed_chunk(chunk)?,
            WireCodec::Json => self.feed_json(chunk),
        };
        if frames
            .iter()
            .any(|frame| frame.len() > self.max_frame_bytes)
            || self.pending_len() > self.max_frame_bytes
        {
            return Err(CratestackError::BadRequest(format!(
                "WebTransport frame exceeds {} bytes",
                self.max_frame_bytes,
            )));
        }
        Ok(frames)
    }

    /// Bytes of an incomplete frame still buffered. Non-zero when the stream
    /// ends means the peer stopped mid-frame.
    pub fn pending_len(&self) -> usize {
        match self.codec {
            WireCodec::Cbor => self.cbor.pending_len(),
            WireCodec::Json => self.json.len(),
        }
    }

    fn feed_json(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        self.json.extend_from_slice(chunk);
        let mut frames = Vec::new();
        while let Some(newline) = self.json.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.json.drain(..=newline).take(newline).collect();
            if !line.iter().all(u8::is_ascii_whitespace) {
                frames.push(line);
            }
        }
        frames
    }
}

#[cfg(test)]
#[path = "tests_codec.rs"]
mod tests;
