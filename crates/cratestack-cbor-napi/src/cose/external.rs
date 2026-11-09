//! A signer whose key never reaches this addon (issue #1010): a
//! non-extractable WebCrypto or Node `KeyObject`, a KMS, a hardware token.
//!
//! The addon has no async runtime to await a JavaScript callback with, so
//! the seal is two steps: [`Envelope::begin_seal`] runs the seal up to the
//! signer, which hands over the complete to-be-signed bytes, and
//! [`Suspended::finish`] resumes it with the signature. The envelope's
//! own `cratestack_cose::ExternalSigner` sits between: it converts a DER
//! signature to raw `r ‖ s`, normalises ESP256 to low-`s` and refuses a
//! signature that does not verify under the public key, exactly as in Rust.
//! JavaScript only produces the signature; the next step is `await`ing the
//! callback in the package's TypeScript wrapper.
//!
//! [`Envelope::begin_seal`]: super::envelope::Envelope::begin_seal

use std::cell::RefCell;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::Poll;

use bytes::Bytes;
use cratestack_core::CratestackError;
use cratestack_cose::{CoseAlg, ExternalSigner};

use super::drive::poll_once;
use super::fail::Fail;

/// What passes between one seal and its signature.
#[derive(Default)]
pub(crate) struct Exchange {
    to_be_signed: Mutex<Option<Vec<u8>>>,
    signature: Mutex<Option<Vec<u8>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned lock only means another seal panicked; the data is a plain
    // `Option` that cannot be left half-written.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

thread_local! {
    /// The exchange of the seal being polled right now. The signer callback
    /// runs inside that poll, on this thread, and reads it.
    static CURRENT: RefCell<Option<Arc<Exchange>>> = const { RefCell::new(None) };
}

struct Current;

impl Current {
    fn set(exchange: &Arc<Exchange>) -> Current {
        CURRENT.with(|current| *current.borrow_mut() = Some(exchange.clone()));
        Current
    }
}

impl Drop for Current {
    fn drop(&mut self) {
        CURRENT.with(|current| *current.borrow_mut() = None);
    }
}

/// The signer. Its key is `public_key`; every signature it returns is the
/// caller's, via [`Suspended::finish`].
pub(crate) fn signer(alg: CoseAlg, public_key: &[u8]) -> Result<ExternalSigner, Fail> {
    let callback = |to_be_signed: Vec<u8>| async move {
        let exchange = CURRENT
            .with(|current| current.borrow().clone())
            .ok_or_else(|| {
                CratestackError::Internal("an external signer ran outside a seal".to_owned())
            })?;
        *lock(&exchange.to_be_signed) = Some(to_be_signed);
        poll_fn(|_| match lock(&exchange.signature).take() {
            Some(signature) => Poll::Ready(Ok(signature)),
            None => Poll::Pending,
        })
        .await
    };
    match alg {
        CoseAlg::Esp256 => ExternalSigner::esp256(public_key, callback),
        CoseAlg::Ed25519 => {
            let public: [u8; 32] = public_key
                .try_into()
                .map_err(|_| Fail::misuse("an Ed25519 public key is 32 bytes"))?;
            ExternalSigner::ed25519(&public, callback)
        }
        _ => return Err(Fail::misuse("an external signer is `esp256` or `ed25519`")),
    }
    .map_err(Fail::from)
}

type SealFuture = Pin<Box<dyn Future<Output = Result<Bytes, CratestackError>> + Send>>;

/// A seal that has reached its signer and waits for the signature.
pub(crate) struct Suspended {
    future: Mutex<Option<SealFuture>>,
    exchange: Arc<Exchange>,
    to_be_signed: Vec<u8>,
}

impl Suspended {
    /// Runs `future` to the signer.
    pub(crate) fn start(mut future: SealFuture) -> Result<Suspended, Fail> {
        let exchange = Arc::new(Exchange::default());
        let polled = {
            let _current = Current::set(&exchange);
            poll_once(future.as_mut())
        };
        match polled {
            Poll::Ready(Err(error)) => Err(error.into()),
            Poll::Ready(Ok(_)) => Err(Fail::misuse("the seal finished without being signed")),
            Poll::Pending => {
                let to_be_signed = lock(&exchange.to_be_signed).take().ok_or_else(|| {
                    Fail::misuse("the seal is waiting for something other than its signer")
                })?;
                Ok(Suspended {
                    future: Mutex::new(Some(future)),
                    exchange,
                    to_be_signed,
                })
            }
        }
    }

    /// The bytes the signer is asked to sign: the complete COSE
    /// `Sig_structure`. Sign exactly these, never a digest of them.
    pub(crate) fn to_be_signed(&self) -> &[u8] {
        &self.to_be_signed
    }

    /// Resume with `signature`. Once: a second call is misuse.
    pub(crate) fn finish(&self, signature: Vec<u8>) -> Result<Vec<u8>, Fail> {
        let mut future = lock(&self.future)
            .take()
            .ok_or_else(|| Fail::misuse("this seal was already finished"))?;
        *lock(&self.exchange.signature) = Some(signature);
        let polled = {
            let _current = Current::set(&self.exchange);
            poll_once(future.as_mut())
        };
        match polled {
            Poll::Ready(sealed) => Ok(sealed?.to_vec()),
            Poll::Pending => Err(Fail::misuse("the seal did not finish after its signature")),
        }
    }
}
