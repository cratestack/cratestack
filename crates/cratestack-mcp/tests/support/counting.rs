//! A provider that counts its calls and otherwise is [`AudienceProvider`],
//! and a request body that arrives in chunks with no `Content-Length`.
//!
//! Several of the guard's refusals are refusals `rmcp` would also make a
//! step later (405, 413, a second `Authorization` value the provider may
//! still read). A status code alone therefore passes with the guard's own
//! step deleted. What the guard's step adds is that the application's
//! provider never runs for such a request, so that is what these count.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use bytes::Bytes;
use cratestack_core::{AuthProvider, CratestackContext, CratestackError, RequestContext};
use http_body::Frame;

use super::token::AudienceProvider;

#[derive(Clone)]
pub struct CountingProvider {
    inner: AudienceProvider,
    calls: Arc<AtomicUsize>,
}

impl CountingProvider {
    pub fn new(audience: &str) -> Self {
        Self {
            inner: AudienceProvider::new(audience),
            calls: Arc::default(),
        }
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl AuthProvider for CountingProvider {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.authenticate(request).await
    }
}

/// `count` data frames of `size` bytes each, and no size hint: what a
/// chunked upload looks like, so only a limit that counts bytes as they
/// arrive can stop it.
pub struct Chunked {
    left: usize,
    chunk: Bytes,
}

impl Chunked {
    pub fn new(count: usize, size: usize) -> Self {
        Self {
            left: count,
            chunk: Bytes::from(vec![b' '; size]),
        }
    }
}

impl http_body::Body for Chunked {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, std::io::Error>>> {
        if self.left == 0 {
            return Poll::Ready(None);
        }
        self.left -= 1;
        Poll::Ready(Some(Ok(Frame::data(self.chunk.clone()))))
    }
}
