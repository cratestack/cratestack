//! gRPC wire framing — the fixed 5-byte length-delimited frame prepended to
//! every gRPC message: 1 byte compression flag + 4 bytes big-endian message
//! length. `docs/design/protobuf.md` §7.3: envelope signing must operate on
//! the *unframed* message bytes, on both seal and verify, so this module is
//! the single place that strips it.
//!
//! Implemented directly from the public gRPC-over-HTTP/2 wire spec
//! (<https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-HTTP2.md#length-prefixed-messages>)
//! rather than reached through tonic's `Codec`/`Decoder` trait chain: this
//! crate does not yet own a tonic service-trait implementation (out of
//! scope for this cut — see the crate README), so there is no live call
//! site inside tonic's own pipeline to intercept bytes at. The frame format
//! is a fixed, public part of the gRPC wire protocol, not a tonic-internal
//! detail, so implementing it standalone here is correct and will remain
//! the right place to plug a future service impl's codec layer into.

const FRAME_HEADER_LEN: usize = 5;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("gRPC frame is too short: got {0} bytes, need at least the 5-byte header")]
    TooShort(usize),
    #[error(
        "gRPC frame declares length {declared} but only {available} bytes are available after the header"
    )]
    LengthMismatch { declared: u32, available: usize },
}

/// Strips the 5-byte gRPC length-prefix frame, returning the raw message
/// bytes underneath. Rejects a frame whose declared length doesn't match
/// what's actually available, rather than silently truncating or padding.
pub fn strip_grpc_frame(framed: &[u8]) -> Result<&[u8], FrameError> {
    if framed.len() < FRAME_HEADER_LEN {
        return Err(FrameError::TooShort(framed.len()));
    }
    let declared_len = u32::from_be_bytes([framed[1], framed[2], framed[3], framed[4]]);
    let body = &framed[FRAME_HEADER_LEN..];
    if body.len() as u64 != u64::from(declared_len) {
        return Err(FrameError::LengthMismatch {
            declared: declared_len,
            available: body.len(),
        });
    }
    Ok(body)
}

/// Adds the 5-byte gRPC length-prefix frame back — the inverse of
/// [`strip_grpc_frame`]. `compressed` is the frame's compression flag
/// (byte 0); CrateStack does not use gRPC's per-message compression today,
/// so every call site is expected to pass `false`.
pub fn frame_grpc_message(message: &[u8], compressed: bool) -> Vec<u8> {
    let mut framed = Vec::with_capacity(FRAME_HEADER_LEN + message.len());
    framed.push(u8::from(compressed));
    framed.extend_from_slice(&(message.len() as u32).to_be_bytes());
    framed.extend_from_slice(message);
    framed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_frame_and_strip() {
        let message = b"hello world";
        let framed = frame_grpc_message(message, false);
        assert_eq!(framed.len(), message.len() + FRAME_HEADER_LEN);
        assert_eq!(strip_grpc_frame(&framed).unwrap(), message);
    }

    #[test]
    fn empty_message_frames_and_strips_cleanly() {
        let framed = frame_grpc_message(&[], false);
        assert_eq!(strip_grpc_frame(&framed).unwrap(), &[] as &[u8]);
    }

    #[test]
    fn rejects_frame_shorter_than_header() {
        assert_eq!(strip_grpc_frame(&[0, 0, 0]), Err(FrameError::TooShort(3)));
    }

    #[test]
    fn rejects_declared_length_mismatch() {
        let mut framed = frame_grpc_message(b"abc", false);
        // Corrupt the declared length to claim 99 bytes follow.
        framed[1..5].copy_from_slice(&99u32.to_be_bytes());
        assert_eq!(
            strip_grpc_frame(&framed),
            Err(FrameError::LengthMismatch {
                declared: 99,
                available: 3,
            })
        );
    }
}
