//! One subscription = one client-opened bidirectional stream
//! (design §6a.2): exactly one `Request` in; `Ready`, items, and one
//! terminal `end` or `err` out; FIN. A client that stops reading has
//! cancelled, and the dispatcher's stream is dropped right there — which is
//! how the subscription's backplane registration is released.

use cratestack_core::{
    CratestackContext, CratestackError, SubscriptionDispatcher, SubscriptionOpen,
};
use futures_util::StreamExt;
use tokio::sync::OwnedSemaphorePermit;
use tracing::Instrument;

use crate::codec::WireCodec;
use crate::frame::{ClientFrame, ClientMessage, ServerFrame, reset_code};
use crate::reader::FrameReader;
use crate::transport::{RecvHalf, SendHalf};

/// How long a finished subscription waits for the client's FIN.
const DRAIN_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

pub(crate) struct StreamCtx<D> {
    pub(crate) dispatcher: D,
    pub(crate) codec: WireCodec,
    pub(crate) max_frame_bytes: usize,
    pub(crate) ctx: CratestackContext,
    /// `None` when the session is already at its subscription limit.
    pub(crate) permit: Option<OwnedSemaphorePermit>,
}

pub(crate) async fn run<D: SubscriptionDispatcher>(
    stream: StreamCtx<D>,
    mut send: SendHalf,
    recv: RecvHalf,
) {
    let mut reader = FrameReader::new(recv, stream.codec, stream.max_frame_bytes);
    let request = reader
        .next::<ClientFrame>()
        .await
        .ok()
        .map(ClientFrame::classify);
    let Some(Ok(ClientMessage::Request { id, op, input })) = request else {
        // Not exactly a `Request` (an `Init`, garbage, or nothing at all).
        reader.stop(reset_code::PROTOCOL);
        send.reset(reset_code::PROTOCOL);
        return;
    };
    let span = tracing::info_span!(
        "cratestack_subscription",
        cratestack_binding = "webtransport",
        cratestack_op_id = %op,
        id,
        end_reason = tracing::field::Empty,
        delivered = tracing::field::Empty,
    );
    let codec = stream.codec;
    let permit = stream.permit;
    let open = SubscriptionOpen {
        op_id: op,
        input,
        ctx: stream.ctx,
    };
    drive(stream.dispatcher, open, id, codec, permit, send)
        .instrument(span)
        .await;
    // Read the client's half to its FIN before letting go. Dropping it
    // earlier sends STOP_SENDING, and a browser whose FIN for the `Request`
    // has not landed yet surfaces that as a rejected `writer.close()` on a
    // subscription that worked — reliably so for one that ends at once.
    reader.drain(DRAIN_GRACE).await;
}

async fn drive<D: SubscriptionDispatcher>(
    dispatcher: D,
    open: SubscriptionOpen,
    id: u64,
    codec: WireCodec,
    permit: Option<OwnedSemaphorePermit>,
    mut send: SendHalf,
) {
    let span = tracing::Span::current();

    let Some(_permit) = permit else {
        let error =
            CratestackError::TooManyRequests("too many subscriptions on this session".to_owned());
        return finish_with(
            &span,
            &mut send,
            codec,
            ServerFrame::err(id, &error),
            "limit",
        )
        .await;
    };

    let opened = tokio::select! {
        opened = dispatcher.open(open) => opened,
        () = send.stopped() => return span_end(&span, "cancel", 0),
    };
    let mut items = match opened {
        Ok(items) => items,
        Err(error) => {
            return finish_with(
                &span,
                &mut send,
                codec,
                ServerFrame::err(id, &error),
                "refused",
            )
            .await;
        }
    };
    if write(&mut send, codec, &ServerFrame::ready(id))
        .await
        .is_err()
    {
        return span_end(&span, "disconnect", 0);
    }

    let mut delivered = 0u64;
    loop {
        let item = tokio::select! {
            item = items.next() => item,
            () = send.stopped() => return span_end(&span, "cancel", delivered),
        };
        let frame = match item {
            Some(Ok(value)) => ServerFrame::next(id, value),
            Some(Err(error)) => {
                span.record("delivered", delivered);
                return finish_with(
                    &span,
                    &mut send,
                    codec,
                    ServerFrame::err(id, &error),
                    "error",
                )
                .await;
            }
            None => {
                span.record("delivered", delivered);
                return finish_with(&span, &mut send, codec, ServerFrame::end(id), "stream_end")
                    .await;
            }
        };
        if write(&mut send, codec, &frame).await.is_err() {
            return span_end(&span, "disconnect", delivered);
        }
        delivered += 1;
    }
}

async fn write(send: &mut SendHalf, codec: WireCodec, frame: &ServerFrame) -> Result<(), ()> {
    let bytes = codec.encode(frame).map_err(|error| {
        tracing::warn!(target: "cratestack", %error, "WebTransport frame failed to encode");
    })?;
    send.write_all(&bytes).await.map_err(|_| ())
}

async fn finish_with(
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

fn span_end(span: &tracing::Span, reason: &str, delivered: u64) {
    span.record("end_reason", reason);
    span.record("delivered", delivered);
}
