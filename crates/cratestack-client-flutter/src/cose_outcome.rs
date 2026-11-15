//! The per-call slot that carries a signer's outcome kind out of a seal.
//!
//! `cratestack-cose` wraps any signer error as an opaque
//! `CratestackError::Internal`, so "the person cancelled" cannot travel
//! inside the error. It travels beside it: [`scoped`] installs a fresh
//! [`Slot`] for the duration of one seal, the signer callback records its
//! outcome there, and the seal's error is replaced by the recorded one.
//!
//! This leans on `ExternalSigner` calling its callback synchronously inside
//! the seal's `poll` (`tests/cose_external_signer.rs` fails if that stops
//! being true). The slot is installed on every poll and removed after it, so
//! a future that hops threads between polls, or two seals interleaved on one
//! thread, each still see their own.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use cratestack_core::CratestackError;

use crate::cose::error::FlutterCoseError;

/// One seal's recorded signer failure.
#[derive(Clone, Default)]
pub(super) struct Slot(Arc<Mutex<Option<FlutterCoseError>>>);

thread_local! {
    static CURRENT: RefCell<Option<Slot>> = const { RefCell::new(None) };
}

/// The slot of the seal being polled on this thread, if any.
pub(super) fn current() -> Option<Slot> {
    CURRENT.with(|current| current.borrow().clone())
}

impl Slot {
    /// Record why the signer did not sign. The first outcome stands.
    pub(super) fn record(&self, error: FlutterCoseError) {
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        held.get_or_insert(error);
    }

    fn take(&self) -> Option<FlutterCoseError> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }
}

/// A future that polls `inner` with `slot` installed as the current one.
struct Scoped<F> {
    slot: Slot,
    inner: Pin<Box<F>>,
}

impl<F: Future> Future for Scoped<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = &mut *self;
        let previous = CURRENT.with(|current| current.replace(Some(this.slot.clone())));
        // Restored even if `inner` panics.
        struct Restore(Option<Slot>);
        impl Drop for Restore {
            fn drop(&mut self) {
                CURRENT.with(|current| *current.borrow_mut() = self.0.take());
            }
        }
        let _restore = Restore(previous);
        this.inner.as_mut().poll(cx)
    }
}

/// Run one seal. A failure the signer callback recorded (cancelled, timed
/// out, failed) is what the caller sees; every other error maps as usual.
pub(super) async fn scoped<T>(
    seal: impl Future<Output = Result<T, CratestackError>>,
) -> Result<T, FlutterCoseError> {
    let slot = Slot::default();
    let result = Scoped {
        slot: slot.clone(),
        inner: Box::pin(seal),
    }
    .await;
    result.map_err(|error| slot.take().unwrap_or_else(|| error.into()))
}

#[cfg(test)]
mod tests {
    use std::future::poll_fn;

    use super::*;
    use crate::cose::error::FlutterCoseErrorKind;
    use crate::cose::sign_outcome::FlutterSignOutcomeKind;

    fn cancelled() -> FlutterCoseError {
        FlutterCoseError::signer(FlutterSignOutcomeKind::Cancelled, String::new()).unwrap()
    }

    #[tokio::test]
    async fn a_recorded_outcome_replaces_the_seal_error() {
        let result: Result<(), _> = scoped(async {
            current().expect("slot").record(cancelled());
            Err(CratestackError::Internal(
                "cose signer unavailable".to_owned(),
            ))
        })
        .await;
        assert_eq!(
            result.unwrap_err().kind,
            FlutterCoseErrorKind::SignerCancelled
        );
    }

    #[tokio::test]
    async fn an_unrecorded_error_maps_as_usual() {
        let result: Result<(), _> =
            scoped(async { Err(CratestackError::Unauthorized("no".to_owned())) }).await;
        assert_eq!(result.unwrap_err(), FlutterCoseError::rejected());
    }

    #[tokio::test]
    async fn the_slot_is_not_current_outside_a_seal() {
        assert!(current().is_none());
        let _: Result<(), _> = scoped(async { Ok(()) }).await;
        assert!(current().is_none());
    }

    #[tokio::test]
    async fn interleaved_seals_keep_their_own_slots() {
        // `a` records, then yields; `b` runs in between and records nothing.
        let a = scoped(async {
            current().expect("slot").record(cancelled());
            let mut yielded = false;
            poll_fn(|cx| {
                if yielded {
                    return Poll::Ready(());
                }
                yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            })
            .await;
            Err::<(), _>(CratestackError::Internal("a".to_owned()))
        });
        let b = scoped(async { Err::<(), _>(CratestackError::Internal("b".to_owned())) });
        let (a, b) = tokio::join!(a, b);
        assert_eq!(a.unwrap_err().kind, FlutterCoseErrorKind::SignerCancelled);
        assert_eq!(b.unwrap_err(), FlutterCoseError::misuse("b"));
    }
}
