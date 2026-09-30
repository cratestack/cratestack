//! What the contract suites share (cratestack#1123): the digests, the
//! table with one op holding an older digest, the routers, and the client
//! side of a request.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use cratestack_core::{
    AcceptedContracts, Binding, CONTRACT_HEADER, ContractSelector, CratestackError,
};
use cratestack_cose::CoseEnvelope;
use http::{HeaderValue, Method};

use super::fixtures::{Hits, REST_ROUTES, rest_router, rpc_router};
use super::support::*;
use crate::envelope_layer::{
    EnvelopeLayer, EnvelopeMode, OpenedRequest, SealContext, Sealed, ServerEnvelope, async_trait,
};

pub(super) const NEW: [u8; 32] = [0x11; 32];
pub(super) const OLD: [u8; 32] = [0x22; 32];
pub(super) const OTHER_OP: [u8; 32] = [0x33; 32];

/// `POST /widgets` accepts its current digest and one older one (newest
/// first); `GET /widgets/{id}` has its own.
pub(super) static WITH_HISTORY: AcceptedContracts = &[
    ("POST /widgets", &[NEW, OLD]),
    ("GET /widgets/{id}", &[OTHER_OP]),
    ("procedure.notify", &[NEW]),
    ("procedure.read", &[OTHER_OP]),
    ("batch", &[NEW]),
];

pub(super) fn rest(table: AcceptedContracts, trials: Option<usize>, hits: &Hits) -> axum::Router {
    rest_under(server_envelope(), table, trials, hits)
}

/// [`rest`] over an envelope that counts every `open_request` it is asked
/// for: the number of parse, key-resolution and verification passes, which
/// `Hits` (the router behind the layer) cannot see, since a refusal never
/// reaches it.
pub(super) fn rest_counting(
    table: AcceptedContracts,
    trials: Option<usize>,
    hits: &Hits,
) -> (axum::Router, Arc<AtomicUsize>) {
    let opens = Arc::new(AtomicUsize::new(0));
    let envelope = CountingEnvelope {
        inner: server_envelope(),
        opens: opens.clone(),
    };
    (rest_under(envelope, table, trials, hits), opens)
}

fn rest_under(
    envelope: impl ServerEnvelope,
    table: AcceptedContracts,
    trials: Option<usize>,
    hits: &Hits,
) -> axum::Router {
    let mut builder = EnvelopeLayer::builder(envelope, AUDIENCE, table)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES);
    if let Some(trials) = trials {
        builder = builder.max_contract_trials(trials);
    }
    rest_router(builder.build().expect("layer"), hits)
}

/// The COSE envelope, counting the requests it is asked to open.
struct CountingEnvelope {
    inner: CoseEnvelope,
    opens: Arc<AtomicUsize>,
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
        self.opens.fetch_add(1, Ordering::SeqCst);
        ServerEnvelope::open_request(&self.inner, body, bind).await
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

pub(super) fn rpc(hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, WITH_HISTORY)
        .policy(EnvelopeMode::Required)
        .rpc("")
        .build()
        .expect("layer");
    rpc_router(layer, hits)
}

pub(super) fn selecting(
    mut req: axum::extract::Request,
    digest: &[u8; 32],
) -> axum::extract::Request {
    let value = ContractSelector::of(digest).to_header_value();
    req.headers_mut().insert(
        CONTRACT_HEADER,
        HeaderValue::from_str(&value).expect("header"),
    );
    req
}

pub(super) async fn post_widgets(
    contract: [u8; 32],
    selector: Option<[u8; 32]>,
) -> (Call, Bytes, Request) {
    let call = Call::new(Method::POST, "/widgets", &[]).contract(contract);
    let sealed = call.seal(PAYLOAD).await;
    let req = cose_request(Method::POST, "/widgets", sealed.clone());
    let req = match selector {
        Some(digest) => selecting(req, &digest),
        None => req,
    };
    (call, sealed, req)
}

pub(super) type Request = axum::extract::Request;
