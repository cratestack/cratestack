//! A layer whose envelope and key resolver count what they are asked for,
//! which `Hits` (the router behind the layer) cannot: a refusal never
//! reaches the router.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use cratestack_core::{AcceptedContracts, Binding, CratestackError, InMemoryNonceStore};
use cratestack_cose::{
    CoseAlg, CoseEnvelope, CoseMode, CoseVerifierResolver, CoseVerifyKey, StaticVerifierResolver,
};

use super::contracts_support::rest_under;
use super::cose_support::{client_signer, server_signer};
use super::fixtures::Hits;
use crate::envelope_layer::{OpenedRequest, SealContext, Sealed, ServerEnvelope, async_trait};

/// What a counting layer saw, which `Hits` (the router behind the layer)
/// cannot: a refusal never reaches the router.
#[derive(Default)]
pub(super) struct Counts {
    passes: AtomicUsize,
    candidates: AtomicUsize,
    resolves: AtomicUsize,
}

impl Counts {
    /// Times the layer asked the envelope to open a request: one per
    /// request that got that far, however many digests it tried. A
    /// request is parsed once per pass.
    pub(super) fn passes(&self) -> usize {
        self.passes.load(Ordering::SeqCst)
    }

    /// Bindings offered across all passes: the most signature
    /// verifications those passes could have run.
    pub(super) fn candidates(&self) -> usize {
        self.candidates.load(Ordering::SeqCst)
    }

    /// Key-resolver lookups.
    pub(super) fn resolves(&self) -> usize {
        self.resolves.load(Ordering::SeqCst)
    }
}

/// [`rest`] over an envelope and a key resolver that count what they are
/// asked for.
pub(super) fn rest_counting(
    table: AcceptedContracts,
    trials: Option<usize>,
    hits: &Hits,
) -> (axum::Router, Arc<Counts>) {
    let counts = Arc::new(Counts::default());
    let resolver = CountingResolver {
        inner: StaticVerifierResolver::new().with_key(client_signer().verify_key()),
        counts: counts.clone(),
    };
    let inner = CoseEnvelope::server(
        CoseMode::Sign1,
        Arc::new(server_signer()),
        Arc::new(resolver),
        Arc::new(InMemoryNonceStore::new()),
    )
    .build()
    .expect("server envelope");
    let envelope = CountingEnvelope {
        inner,
        counts: counts.clone(),
    };
    (rest_under(envelope, table, trials, hits), counts)
}

struct CountingResolver {
    inner: StaticVerifierResolver,
    counts: Arc<Counts>,
}

#[async_trait]
impl CoseVerifierResolver for CountingResolver {
    async fn resolve(
        &self,
        kid: &[u8],
        alg: CoseAlg,
    ) -> Result<Vec<CoseVerifyKey>, CratestackError> {
        self.counts.resolves.fetch_add(1, Ordering::SeqCst);
        self.inner.resolve(kid, alg).await
    }
}

/// The COSE envelope, counting the requests it is asked to open.
struct CountingEnvelope {
    inner: CoseEnvelope,
    counts: Arc<Counts>,
}

#[async_trait]
impl ServerEnvelope for CountingEnvelope {
    fn media_type(&self) -> &'static str {
        ServerEnvelope::media_type(&self.inner)
    }

    fn is_envelope_content_type(&self, content_type: &str) -> bool {
        ServerEnvelope::is_envelope_content_type(&self.inner, content_type)
    }

    async fn open_request(
        &self,
        body: Bytes,
        bind: &Binding<'_>,
    ) -> Result<OpenedRequest, CratestackError> {
        self.counts.passes.fetch_add(1, Ordering::SeqCst);
        self.counts.candidates.fetch_add(1, Ordering::SeqCst);
        ServerEnvelope::open_request(&self.inner, body, bind).await
    }

    async fn open_request_any(
        &self,
        body: Bytes,
        binds: &[Binding<'_>],
    ) -> Result<(OpenedRequest, usize), CratestackError> {
        self.counts.passes.fetch_add(1, Ordering::SeqCst);
        self.counts
            .candidates
            .fetch_add(binds.len(), Ordering::SeqCst);
        ServerEnvelope::open_request_any(&self.inner, body, binds).await
    }

    async fn seal_response(
        &self,
        payload: &[u8],
        bind: &Binding<'_>,
        context: &SealContext,
    ) -> Result<Sealed, CratestackError> {
        ServerEnvelope::seal_response(&self.inner, payload, bind, context).await
    }
}
