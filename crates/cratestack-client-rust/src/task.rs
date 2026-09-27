//! Target-split task spawning and boxed futures (cratestack#1104).
//!
//! On native targets reqwest's futures are `Send` and the streamed-response
//! pump runs on the caller's tokio runtime, exactly as before. On
//! `wasm32-unknown-unknown` reqwest goes through the browser's `fetch`, whose
//! futures hold JS values and are never `Send`, and there is no tokio runtime
//! to spawn onto — the pump runs on the browser's own event loop instead.
//! Keeping both halves here means each call site names one `spawn` and one
//! `BoxFuture`, and the native signatures stay byte-for-byte what they were.

use std::future::Future;
use std::pin::Pin;

/// A boxed future: `Send` on native targets, not `Send` on wasm32.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
/// A boxed future: `Send` on native targets, not `Send` on wasm32.
#[cfg(target_arch = "wasm32")]
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Detach `future` onto the caller's tokio runtime.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(future);
}

/// Detach `future` onto the browser's event loop.
#[cfg(target_arch = "wasm32")]
pub(crate) fn spawn<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(future);
}
