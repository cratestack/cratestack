//! The target split behind [`CoseSigner`](crate::CoseSigner) and
//! [`CoseVerifierResolver`](crate::CoseVerifierResolver) (cratestack#1007).
//!
//! Natively a signer is shared between threads, so it and its futures must be
//! `Send + Sync`. On `wasm32` there is one thread and a browser future is
//! never `Send`, so requiring it would make a WebCrypto or keystore-callback
//! signer impossible to write. One definition with a target-dependent
//! supertrait keeps the two halves from drifting, where two copies of each
//! trait would not.

use std::future::Future;
use std::pin::Pin;

/// `Send + Sync` natively, nothing on `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSendSync: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync + ?Sized> MaybeSendSync for T {}

/// `Send + Sync` natively, nothing on `wasm32`.
#[cfg(target_arch = "wasm32")]
pub trait MaybeSendSync {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSendSync for T {}

/// `Send` natively, nothing on `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + ?Sized> MaybeSend for T {}

/// `Send` natively, nothing on `wasm32`.
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSend for T {}

/// A boxed future, `Send` natively and not on `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
/// A boxed future, `Send` natively and not on `wasm32`.
#[cfg(target_arch = "wasm32")]
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
