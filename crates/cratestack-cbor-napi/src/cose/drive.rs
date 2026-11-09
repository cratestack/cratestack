//! Driving a future by hand, with no executor.
//!
//! Every future in this binding is ready on its first poll, except a seal
//! that is waiting for an external signer (`external.rs`). The addon pulls
//! no async runtime (no `tokio`) for that.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

/// One poll with a waker that does nothing.
pub(crate) fn poll_once<T>(future: Pin<&mut (dyn Future<Output = T> + '_)>) -> Poll<T> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

/// The output of a future that must be ready on its first poll.
pub(crate) fn ready<T>(future: impl Future<Output = T>) -> Option<T> {
    let mut future = std::pin::pin!(future);
    match poll_once(future.as_mut()) {
        Poll::Ready(output) => Some(output),
        Poll::Pending => None,
    }
}
