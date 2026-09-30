//! What the contract suites share (cratestack#1123): the digests, the
//! table with one op holding an older digest, the routers, and the client
//! side of a request.

use bytes::Bytes;
use cratestack_core::{AcceptedContracts, CONTRACT_HEADER, ContractSelector};
use http::{HeaderValue, Method};

use super::fixtures::{Hits, REST_ROUTES, rest_router, rpc_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

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
    let mut builder = EnvelopeLayer::builder(server_envelope(), AUDIENCE, table)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES);
    if let Some(trials) = trials {
        builder = builder.max_contract_trials(trials);
    }
    rest_router(builder.build().expect("layer"), hits)
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
