//! One subscription on a socket (design §6.4): open it through the
//! dispatcher, then `Ready`, its items, and exactly one `end` or `err`.
//! Frames go to the socket's outbound queue; a queue too full to take one
//! closes the socket (1013) rather than drop it silently.

use cratestack_core::subscription_wire::ServerFrame;
use cratestack_core::{SubscriptionDispatcher, SubscriptionOpen};
use futures_util::StreamExt;
use tokio::sync::mpsc;

/// What a subscription task tells its socket.
pub(crate) enum Outbound {
    Frame(ServerFrame),
    /// Subscription `id` ended; its slot is free.
    Done(u64),
}

/// Queues `frame`; `false` when the queue is full or gone.
pub(crate) fn queue(out: &mpsc::Sender<Outbound>, outbound: Outbound) -> bool {
    out.try_send(outbound).is_ok()
}

pub(crate) async fn run<D: SubscriptionDispatcher>(
    dispatcher: D,
    open: SubscriptionOpen,
    id: u64,
    out: mpsc::Sender<Outbound>,
    overflow: std::sync::Arc<tokio::sync::Notify>,
) {
    let send = |frame| {
        let queued = queue(&out, Outbound::Frame(frame));
        if !queued {
            overflow.notify_one();
        }
        queued
    };
    match dispatcher.open(open).await {
        Err(error) => {
            send(ServerFrame::err(id, &error));
        }
        Ok(mut items) => {
            if send(ServerFrame::ready(id)) {
                loop {
                    let frame = match items.next().await {
                        Some(Ok(item)) => ServerFrame::next(id, item),
                        Some(Err(error)) => {
                            send(ServerFrame::err(id, &error));
                            break;
                        }
                        None => {
                            send(ServerFrame::end(id));
                            break;
                        }
                    };
                    if !send(frame) {
                        break;
                    }
                }
            }
        }
    }
    let _ = out.try_send(Outbound::Done(id));
}
