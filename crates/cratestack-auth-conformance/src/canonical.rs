//! What the kit sends and what the provider saw: the canonical request of
//! ADR 0021 §3, and a provider wrapper that records it.

use std::sync::{Arc, Mutex};

use cratestack::{AuthProvider, CratestackContext, Credential, RequestContext};

/// A transport the kit drives the provider through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Transport {
    /// A REST procedure call, `POST /$procs/whoAmI`.
    Rest,
    /// An RPC unary call, `POST /rpc/procedure.whoAmI`.
    RpcUnary,
    /// An RPC batch of two frames, `POST /rpc/batch`.
    RpcBatch,
    /// An MCP `tools/call` over Streamable HTTP, `POST /mcp`, with
    /// `CredentialPresence::ask_provider()`.
    Mcp,
}

/// The request the kit is about to send, as the provider must be given it:
/// the declared method and route (without `mount`), the query the provider
/// should see, and the exact body bytes. A signer reads it to produce the
/// credential headers for this request.
#[derive(Debug, Clone, PartialEq)]
pub struct Canonical {
    /// Which transport carries it.
    pub transport: Transport,
    /// The prefix the router is mounted under (`""` or `"/kit"`). It is part
    /// of the URI the kit sends, never of `path`.
    pub mount: &'static str,
    /// The declared method.
    pub method: &'static str,
    /// The declared route, relative to the router's root.
    pub path: &'static str,
    /// The query the provider must see.
    pub query: Option<&'static str>,
    /// The exact body bytes.
    pub body: Vec<u8>,
}

/// One `authenticate` call, as the provider saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) query: Option<String>,
    pub(crate) body: Vec<u8>,
}

impl Seen {
    pub(crate) fn expected(canonical: &Canonical) -> Self {
        Self {
            method: canonical.method.to_owned(),
            path: canonical.path.to_owned(),
            query: canonical.query.map(str::to_owned),
            body: canonical.body.clone(),
        }
    }
}

/// The provider under test, recording each `authenticate` call.
#[derive(Clone)]
pub(crate) struct Recording<P> {
    inner: P,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl<P> Recording<P> {
    pub(crate) fn new(inner: P) -> Self {
        Self {
            inner,
            seen: Arc::default(),
        }
    }

    /// Every call since the last `take`, cleared.
    pub(crate) fn take(&self) -> Vec<Seen> {
        std::mem::take(&mut *self.seen.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl<P: AuthProvider> AuthProvider for Recording<P> {
    type Error = P::Error;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, P::Error> {
        self.seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Seen {
                method: request.method.to_owned(),
                path: request.path.to_owned(),
                query: request.query.map(str::to_owned),
                body: request.body.to_vec(),
            });
        self.inner.authenticate(request).await
    }

    fn credential<'r>(&self, request: &RequestContext<'r>) -> Credential<'r> {
        self.inner.credential(request)
    }
}
