//! The stream's writes, and the span fields they close it with.

use crate::codec::WireCodec;
use crate::frame::ServerFrame;
use crate::transport::SendHalf;

pub(super) async fn write(
    send: &mut SendHalf,
    codec: WireCodec,
    frame: &ServerFrame,
) -> Result<(), ()> {
    let bytes = codec.encode(frame).map_err(|error| {
        tracing::warn!(target: "cratestack", %error, "WebTransport frame failed to encode");
    })?;
    send.write_all(&bytes).await.map_err(|_| ())
}

pub(super) async fn finish_with(
    span: &tracing::Span,
    send: &mut SendHalf,
    codec: WireCodec,
    frame: ServerFrame,
    reason: &str,
) {
    if write(send, codec, &frame).await.is_ok() {
        send.finish();
    }
    span.record("end_reason", reason);
}

pub(super) fn span_end(span: &tracing::Span, reason: &str, delivered: u64) {
    span.record("end_reason", reason);
    span.record("delivered", delivered);
}
