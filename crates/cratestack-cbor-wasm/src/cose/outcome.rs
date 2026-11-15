//! The per-call slot that carries a keystore signer's outcome kind out of a
//! seal (cratestack#1151), for the Dart web backend.
//!
//! `cratestack-cose` wraps any signer error as an opaque
//! `CratestackError::Internal`, so "the person cancelled" cannot travel
//! inside the error. [`scoped`] installs a fresh [`Slot`] for the duration of
//! one seal, [`call_sign`](super::external) records the outcome in it, and the
//! seal's error is replaced by the recorded `{ code, message }`.
//!
//! The slot is installed on every poll, so two seals interleaved on the one
//! JS thread (a seal suspended on a keystore prompt while another runs) each
//! keep their own.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use cratestack_core::CratestackError;
use js_sys::Reflect;
use wasm_bindgen::{JsCast, JsValue};

use super::error::{self, from_error};

/// One seal's recorded signer failure: an error `code` and its message.
#[derive(Clone, Default)]
pub(super) struct Slot(Rc<RefCell<Option<(&'static str, String)>>>);

thread_local! {
    static CURRENT: RefCell<Option<Slot>> = const { RefCell::new(None) };
}

/// The slot of the seal being polled, if any.
pub(super) fn current() -> Option<Slot> {
    CURRENT.with(|current| current.borrow().clone())
}

impl Slot {
    /// Record a tagged resolution of the signing callback. The first one
    /// stands.
    pub(super) fn record(&self, code: &'static str, message: String) {
        self.0.borrow_mut().get_or_insert((code, message));
    }
}

/// The outcome a callback resolved to, if `settled` is the tagged object
/// `{ outcome: "cancelled" | "timedOut" | "failed", message?: string }`:
/// its error `code` and message. `None` for anything else.
pub(super) fn parse(settled: &JsValue) -> Option<(&'static str, String)> {
    if !settled.is_object() {
        return None;
    }
    let tag = Reflect::get(settled, &"outcome".into()).ok()?.as_string()?;
    let code = match tag.as_str() {
        "cancelled" => error::SIGNER_CANCELLED,
        "timedOut" => error::SIGNER_TIMED_OUT,
        "failed" => error::SIGNER_FAILED,
        _ => return None,
    };
    // Only a failure carries a message; the other two are the same everywhere.
    let message = match code {
        error::SIGNER_FAILED => Reflect::get(settled, &"message".into())
            .ok()
            .and_then(|message| message.dyn_into::<js_sys::JsString>().ok())
            .map(String::from)
            .unwrap_or_default(),
        _ => String::new(),
    };
    Some((code, message))
}

struct Scoped<F> {
    slot: Slot,
    inner: Pin<Box<F>>,
}

impl<F: Future> Future for Scoped<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = &mut *self;
        let previous = CURRENT.with(|current| current.replace(Some(this.slot.clone())));
        let polled = this.inner.as_mut().poll(cx);
        CURRENT.with(|current| *current.borrow_mut() = previous);
        polled
    }
}

/// Run one seal. A failure the signer callback recorded is what rejects;
/// every other error maps as usual.
pub(super) async fn scoped<T>(
    seal: impl Future<Output = Result<T, CratestackError>>,
) -> Result<T, JsValue> {
    let slot = Slot::default();
    let result = Scoped {
        slot: slot.clone(),
        inner: Box::pin(seal),
    }
    .await;
    result.map_err(|e| match slot.0.take() {
        Some((code, message)) => error::object(code, &message),
        None => from_error(e),
    })
}
