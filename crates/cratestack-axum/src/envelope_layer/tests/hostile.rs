//! Hostile or careless plug-ins against the reshaped seams (API review,
//! 2026-09-26), with the toy envelope, so these run with the `envelope`
//! feature alone: a sealed body is never labelled as plain, the seal
//! context comes back to its own request, a principal mapper can refuse
//! but not leak, and the builder refuses what would bind nothing.

use std::sync::Arc;

use axum::body::Body;
use cratestack_core::CratestackError;
use http::{Method, StatusCode, header};

use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use super::toy::{Toy, toy_request};
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode, ServerEnvelope, VerifiedRequest};

fn layer(envelope: impl ServerEnvelope) -> crate::envelope_layer::EnvelopeLayerBuilder {
    EnvelopeLayer::builder(envelope, AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES)
}

fn signed() -> axum::extract::Request {
    toy_request(Method::POST, "/widgets", "application/cose", b"TOY:x")
}

#[tokio::test]
async fn an_envelope_that_labels_its_seal_as_plain_cbor_is_never_sent() {
    for label in ["application/cbor", "application/json", "not a\nheader"] {
        let hits = Hits::default();
        let toy = Toy {
            sealed_as: Some(label),
            ..Toy::default()
        };
        let answer = send(
            &rest_router(layer(toy).build().expect("layer"), &hits),
            signed(),
        )
        .await;
        assert_eq!(
            answer.status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{label:?}"
        );
        assert!(
            !answer.body.starts_with(b"TOYRESP"),
            "{label:?}: the sealed body leaked"
        );
        assert_eq!(hits.get(), 1, "it ran; only its answer is withheld");
    }
}

#[tokio::test]
async fn the_seal_context_comes_back_to_its_own_request_and_arc_is_an_envelope() {
    let hits = Hits::default();
    let shared: Arc<dyn ServerEnvelope> = Arc::new(Toy::default());
    let answer = send(
        &rest_router(layer(shared).build().expect("layer"), &hits),
        signed(),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    // `TOYRESP:<route>!=<opened as>:..` if the context were lost or swapped.
    assert_eq!(answer.body.as_ref(), b"TOYRESP:/widgets:200:x");
}

#[tokio::test]
async fn a_principal_mapper_refusal_is_the_unsigned_401_and_a_failure_a_sealed_500() {
    fn refuse(_: &VerifiedRequest<'_>) -> Result<String, CratestackError> {
        Err(CratestackError::Unauthorized(
            "device revoked: secret".to_owned(),
        ))
    }
    fn fail(_: &VerifiedRequest<'_>) -> Result<String, CratestackError> {
        Err(CratestackError::Unavailable(
            "directory down: secret".to_owned(),
        ))
    }
    let hits = Hits::default();
    let router = rest_router(
        layer(Toy::default())
            .principal_mapper(refuse)
            .build()
            .expect("layer"),
        &hits,
    );
    let answer = send(&router, signed()).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert!(!answer.body.starts_with(b"TOYRESP"), "D4: unsigned");
    assert!(!String::from_utf8_lossy(&answer.body).contains("secret"));
    let router = rest_router(
        layer(Toy::default())
            .principal_mapper(fail)
            .build()
            .expect("layer"),
        &hits,
    );
    let answer = send(&router, signed()).await;
    assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(answer.body.starts_with(b"TOYRESP:/widgets:500:"), "sealed");
    assert!(!String::from_utf8_lossy(&answer.body).contains("secret"));
    assert_eq!(hits.get(), 0, "neither ran the handler");
}

#[tokio::test]
async fn a_body_that_fails_to_arrive_is_a_400_not_a_413() {
    let hits = Hits::default();
    let stream = futures_util::stream::iter([
        Ok(bytes::Bytes::from_static(b"TOY:")),
        Err(std::io::Error::other("connection reset")),
    ]);
    let req = http::Request::builder()
        .method(Method::POST)
        .uri("/widgets")
        .header(header::CONTENT_TYPE, "application/cose")
        .body(Body::from_stream(stream))
        .expect("request");
    let answer = send(
        &rest_router(layer(Toy::default()).build().expect("layer"), &hits),
        req,
    )
    .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert_eq!(hits.get(), 0);
}

#[test]
fn the_builder_refuses_an_unclaimed_media_type_and_an_empty_route_table() {
    let unclaimed = Toy {
        names: Some("application/x-toy"),
        ..Toy::default()
    };
    assert!(
        layer(unclaimed).build().is_err(),
        "names a type it does not claim"
    );
    let claimed = Toy {
        names: Some("application/x-toy"),
        claims_toy: true,
        ..Toy::default()
    };
    assert!(layer(claimed).build().is_ok(), "claims it");
    let bad_header = Toy {
        names: Some("application/cose\n"),
        ..Toy::default()
    };
    assert!(layer(bad_header).build().is_err(), "not a header value");
    let empty = EnvelopeLayer::builder(Toy::default(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .rest("", &[])
        .build();
    assert!(empty.is_err(), "rest(..) with no routes binds nothing");
}
