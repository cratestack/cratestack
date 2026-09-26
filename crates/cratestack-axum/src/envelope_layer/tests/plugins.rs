//! One custom implementation per extension point, each doing what the
//! trait promises it can.

use axum::body::Body;
use cratestack_cose::{NONCE_HEADER, RequestNonce, request_digest};
use http::{Method, StatusCode, header};

use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use super::toy::{TOY, Toy, toy_request};
use crate::envelope_layer::{
    BindingResolver, EnvelopeLayer, EnvelopeMode, PolicyRequest, Resolution, ResolvedRoute,
    RouteRequest, UnsignedRequest, VerifiedRequest,
};

#[tokio::test]
async fn a_custom_envelope_with_its_own_media_type() {
    let hits = Hits::default();
    let toy = Toy {
        claims_toy: true,
        ..Toy::default()
    };
    let layer = EnvelopeLayer::builder(toy, AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    let answer = send(
        &rest_router(layer, &hits),
        toy_request(Method::POST, "/widgets", TOY, b"TOY:hello"),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.content_type(), TOY);
    assert_eq!(answer.body.as_ref(), b"TOYRESP:/widgets:200:hello");
    assert_eq!(
        answer.seen("x-seen-principal"),
        format!("cose:{}", "42".repeat(32))
    );
}

#[tokio::test]
async fn a_per_op_policy_closure() {
    let hits = Hits::default();
    let policy = |request: &PolicyRequest<'_>| {
        if request.method() == Method::GET && request.op() == "/widgets/{id}" {
            EnvelopeMode::Optional
        } else {
            EnvelopeMode::Required
        }
    };
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(policy)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    let router = rest_router(layer, &hits);
    let read = send(&router, plain_request(Method::GET, "/widgets/1", b"")).await;
    assert_eq!(read.status, StatusCode::OK);
    let write = send(&router, plain_request(Method::POST, "/widgets", PAYLOAD)).await;
    assert_eq!(write.status, StatusCode::UNAUTHORIZED);
}

struct EverythingIsOneOp;

impl BindingResolver for EverythingIsOneOp {
    fn resolve(&self, request: &RouteRequest<'_>) -> Resolution {
        match request.matched_path() {
            Some(_) => Resolution::Op(ResolvedRoute::new("op.everything", Vec::new())),
            None => Resolution::Unresolved,
        }
    }
}

#[tokio::test]
async fn a_custom_binding_resolver() {
    let hits = Hits::default();
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .binding_resolver(EverythingIsOneOp)
        .build()
        .expect("layer");
    let call = Call::new(Method::POST, "op.everything", &[]);
    let sealed = call.seal(PAYLOAD).await;
    let answer = send(
        &rest_router(layer, &hits),
        cose_request(Method::POST, "/widgets", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
}

#[tokio::test]
async fn a_custom_principal_mapper() {
    let hits = Hits::default();
    let mapper =
        |verified: &VerifiedRequest<'_>| Ok(format!("tenant-a:alg{}", verified.signer().alg()));
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES)
        .principal_mapper(mapper)
        .build()
        .expect("layer");
    let sealed = Call::new(Method::POST, "/widgets", &[]).seal(PAYLOAD).await;
    let answer = send(
        &rest_router(layer, &hits),
        cose_request(Method::POST, "/widgets", sealed),
    )
    .await;
    assert_eq!(answer.seen("x-seen-principal"), "tenant-a:alg-19");
}

#[tokio::test]
async fn a_custom_response_seal_policy() {
    let hits = Hits::default();
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Optional)
        .rest("", &REST_ROUTES)
        .response_seal_policy(|request: &UnsignedRequest<'_>| request.method() == Method::GET)
        .build()
        .expect("layer");
    let nonce = RequestNonce::from_bytes([3; 16]).to_header_value();
    let req = http::Request::builder()
        .uri("/widgets/1")
        .header(NONCE_HEADER, nonce)
        .header(header::ACCEPT, "application/cbor")
        .body(Body::empty())
        .expect("request");
    let answer = send(&rest_router(layer, &hits), req).await;
    assert!(
        answer.is_sealed(),
        "sealed without asking for application/cose"
    );
}

/// The default mapper's prefix is configurable (second-review nit): a toy,
/// non-COSE envelope's signers need not be `cose:`.
#[tokio::test]
async fn the_thumbprint_principal_takes_another_prefix() {
    let hits = Hits::default();
    let toy = Toy {
        claims_toy: true,
        ..Toy::default()
    };
    let layer = EnvelopeLayer::builder(toy, AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .principal_mapper(crate::envelope_layer::ThumbprintPrincipal::with_prefix(
            "toy:",
        ))
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    let answer = send(
        &rest_router(layer, &hits),
        toy_request(Method::POST, "/widgets", TOY, b"TOY:hello"),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(
        answer.seen("x-seen-principal"),
        format!("toy:{}", "42".repeat(32))
    );
}
