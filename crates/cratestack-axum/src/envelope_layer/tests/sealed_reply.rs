//! `UnsignedRequestSealedReply` (cratestack#1313): an unsigned request is
//! accepted only when its reply can be sealed. Anything else is refused with
//! an unsigned `406` before the handler runs, on REST and RPC alike; under
//! `Optional` the same requests ran the handler and were answered plain.

use axum::body::Body;
use cratestack_cose::{NONCE_HEADER, RequestNonce, request_digest, request_digest_unsigned};
use http::{Method, StatusCode, header};

use super::fixtures::{Hits, REST_ROUTES, rest_router, rpc_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

const MODE: EnvelopeMode = EnvelopeMode::UnsignedRequestSealedReply;

fn builder() -> crate::envelope_layer::EnvelopeLayerBuilder {
    EnvelopeLayer::builder(server_envelope(), AUDIENCE, CONTRACTS).policy(MODE)
}

fn rest(hits: &Hits) -> axum::Router {
    rest_router(
        builder().rest("", &REST_ROUTES).build().expect("layer"),
        hits,
    )
}

fn rpc(hits: &Hits) -> axum::Router {
    rpc_router(builder().rpc("").build().expect("layer"), hits)
}

fn unsigned(
    method: Method,
    uri: &str,
    nonces: &[&str],
    accept: Option<&str>,
) -> axum::extract::Request {
    let mut builder = http::Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/cbor");
    for nonce in nonces {
        builder = builder.header(NONCE_HEADER, *nonce);
    }
    if let Some(accept) = accept {
        builder = builder.header(header::ACCEPT, accept);
    }
    builder.body(Body::from(PAYLOAD)).expect("request")
}

/// Every way the reply could not be sealed.
fn unsealable(nonce: &str) -> Vec<(Vec<String>, Option<&'static str>)> {
    let one = vec![nonce.to_owned()];
    vec![
        (vec![], Some(SIGN1)),
        (vec![], None),
        (vec!["not-a-nonce".to_owned()], Some(SIGN1)),
        (vec![nonce.to_owned(), nonce.to_owned()], Some(SIGN1)),
        (one.clone(), None),
        (one.clone(), Some("application/cbor")),
        (one, Some("application/cose;q=0, application/cbor")),
    ]
}

async fn assert_refused(router: &axum::Router, method: Method, uri: &str, hits: &Hits) {
    let nonce = RequestNonce::from_bytes([9; 16]).to_header_value();
    for (nonces, accept) in unsealable(&nonce) {
        let nonces: Vec<&str> = nonces.iter().map(String::as_str).collect();
        let answer = send(router, unsigned(method.clone(), uri, &nonces, accept)).await;
        let case = format!("{uri} {nonces:?} {accept:?}");
        assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE, "{case}");
        assert!(!answer.is_sealed(), "{case}");
        assert_eq!(hits.get(), 0, "the handler ran: {case}");
    }
}

#[tokio::test]
async fn rest_refuses_an_unsealable_unsigned_request_before_the_handler_runs() {
    let hits = Hits::default();
    assert_refused(&rest(&hits), Method::POST, "/widgets", &hits).await;
}

#[tokio::test]
async fn rpc_refuses_an_unsealable_unsigned_request_before_the_handler_runs() {
    let hits = Hits::default();
    assert_refused(&rpc(&hits), Method::POST, "/rpc/procedure.notify", &hits).await;
}

#[tokio::test]
async fn a_nonce_bound_unsigned_request_is_answered_sealed_on_both_transports() {
    let hits = Hits::default();
    let nonce = RequestNonce::from_bytes([9; 16]);
    let header = nonce.to_header_value();
    for (router, uri, op, params) in [
        (rest(&hits), "/widgets", "/widgets", &[][..]),
        (
            rpc(&hits),
            "/rpc/procedure.notify",
            "procedure.notify",
            &[][..],
        ),
    ] {
        let answer = send(
            &router,
            unsigned(Method::POST, uri, &[&header], Some(SIGN1)),
        )
        .await;
        assert!(answer.is_sealed(), "{uri} {}", answer.status);
        Call::new(Method::POST, op, params)
            .open(
                request_digest_unsigned(&nonce, PAYLOAD),
                answer.status,
                answer.body,
            )
            .await
            .expect("bound to its nonce and body");
    }
    assert_eq!(hits.get(), 2);
}

#[tokio::test]
async fn a_signed_request_is_still_opened_and_answered_sealed() {
    let hits = Hits::default();
    let call = Call::new(Method::POST, "/widgets", &[]);
    let sealed = call.seal(PAYLOAD).await;
    let answer = send(
        &rest(&hits),
        cose_request(Method::POST, "/widgets", sealed.clone()),
    )
    .await;
    assert_eq!(answer.seen("x-seen-principal"), CLIENT_PRINCIPAL);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
}

/// A batch runs under the strictest mode of its frames: one frame that owes
/// a sealed reply makes the whole unsigned batch refuse without a nonce.
#[tokio::test]
async fn a_plain_batch_with_a_sealed_reply_frame_is_refused_without_a_nonce() {
    let hits = Hits::default();
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, CONTRACTS)
        .policy(
            |request: &crate::envelope_layer::PolicyRequest<'_>| match request.op() {
                "procedure.enroll" => MODE,
                _ => EnvelopeMode::Optional,
            },
        )
        .rpc("")
        .build()
        .expect("layer");
    let router = rpc_router(layer, &hits);
    let req = http::Request::post("/rpc/batch")
        .header(header::CONTENT_TYPE, "application/cbor")
        .body(Body::from(batch_frames(&[
            "procedure.read",
            "procedure.enroll",
        ])))
        .expect("request");
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
    assert!(!answer.is_sealed());
    assert_eq!(hits.get(), 0);
}
