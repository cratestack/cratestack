//! gRPC wire framing — the fixed 5-byte length-delimited frame prepended to
//! every gRPC message: 1 byte compression flag + 4 bytes big-endian message
//! length. `docs/design/protobuf.md` §7.3: envelope signing must operate on
//! the *unframed* message bytes, on both seal and verify, so this module is
//! the single place that strips it.
//!
//! Implemented directly from the public gRPC-over-HTTP/2 wire spec
//! (<https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-HTTP2.md#length-prefixed-messages>)
//! rather than reached through tonic's `Codec`/`Decoder` trait chain: the
//! generated tonic service impl (`cratestack-macros`' `grpc` feature)
//! doesn't intercept raw wire bytes either — see the "Known gap" note on
//! its `service.rs` module doc — so there is no live call site inside
//! tonic's own pipeline to intercept bytes at. The frame format is a
//! fixed, public part of the gRPC wire protocol, not a tonic-internal
//! detail, so implementing it standalone here is correct and will remain
//! the right place to plug a future service impl's codec layer into.
//!
//! ## gRPC-Web (ticket #172) does not sharpen this module's gap further
//!
//! `docs/design/protobuf.md` §7.4 flags two gRPC-Web-specific wire
//! concerns for anything that touches raw framed bytes: (1) gRPC-Web
//! appends a *trailer frame* to the response body (flagged via the frame
//! header's MSB, `0x80`) that must never be treated as message bytes, and
//! (2) gRPC-Web's text mode base64-encodes the whole body, so byte-level
//! code must operate on *decoded* bytes in both modes. Neither concern
//! reaches this module in practice, because `tonic_web::GrpcWebLayer`
//! (applied by `cratestack::grpc::apply_grpc_web`, mounted by every
//! macro-generated `into_router`) sits in front of the tonic service as
//! an HTTP-body-level translation layer:
//!
//! - On the request path (the only path anything in this crate
//!   canonicalizes — there is no framework-level response-signing step,
//!   see `canonical`'s module doc), `GrpcWebLayer` decodes base64 (when
//!   the client used `application/grpc-web-text+proto`) *before* handing
//!   the body to the wrapped tonic service. There is no trailer frame on
//!   the request side to begin with — gRPC-Web only supports unary and
//!   server-streaming calls, so a request body is always exactly one
//!   message frame, never a trailer frame.
//! - On the response path, `GrpcWebLayer` is what *adds* the trailer
//!   frame (translating tonic's real HTTP/2 trailers into it) and
//!   base64-encodes the whole thing when applicable — again strictly
//!   downstream of anything this crate's functions would see.
//!
//! So by the time [`strip_grpc_frame`] or [`crate::canonical::
//! grpc_canonical_request_string`] runs — inside the generated tonic
//! service, behind `GrpcWebLayer` — the bytes are always plain,
//! binary-mode gRPC framing: one 5-byte header, no trailer frame, no
//! base64, regardless of which content-type the original client spoke.
//! gRPC-Web does not add a new byte-level case this module needs to
//! handle; it only holds as long as every mount point uses
//! `apply_grpc_web` (or composes `GrpcWebLayer` in the same position) —
//! a hand-rolled mount that skips it, or that runs custom byte-level code
//! *outside* the layer, would need to handle both concerns itself.

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
