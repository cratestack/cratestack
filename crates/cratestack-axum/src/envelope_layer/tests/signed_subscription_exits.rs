//! The exits of a signed subscription open (cratestack#1378): what the layer
//! lets out unsealed, and what it never does. The unsealed exit is only a
//! successful, marked stream in the asked type, whatever the mode; every
//! other answer is sealed.

use http::{Method, StatusCode};

use super::fixtures::Hits;
use super::signed_subscription_support::*;
use super::support::*;
use crate::envelope_layer::EnvelopeMode;

const BOTH: &str = "application/cbor-seq, application/cose; cose-type=\"cose-sign1\"";

/// A handler that answers `500` in the asked stream type, marked as a stream,
/// is a failure, not a stream: the answer is sealed, never passed plain.
#[tokio::test]
async fn a_failed_stream_in_the_asked_type_is_sealed_not_passed_plain() {
    let hits = Hits::default();
    let router = router(
        builder().build().expect("layer"),
        &hits,
        Serve::FailedStream,
    );
    let (request, ..) = open(Method::GET, MARKED, &[CBOR_SEQ]).await;
    let answer = send(&router, request).await;
    assert_eq!(hits.get(), 1, "the handler ran");
    assert!(
        answer.is_sealed(),
        "{} {}",
        answer.status,
        answer.content_type()
    );
    assert_eq!(answer.seen("cratestack-frames"), "<absent>");
}

/// The unsigned exit sets its own headers: whatever `Cratestack-Payload-Type`
/// the handler set is stripped, as it describes a payload that is not there.
#[tokio::test]
async fn the_unsigned_exit_strips_a_handler_set_payload_type() {
    let hits = Hits::default();
    let router = router(
        builder().build().expect("layer"),
        &hits,
        Serve::PayloadTyped,
    );
    let (request, ..) = open(Method::GET, MARKED, &[BOTH]).await;
    let answer = send(&router, request).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.seen("cratestack-frames"), "unsigned");
    assert_eq!(answer.seen("cratestack-payload-type"), "<absent>");
}

/// The unsigned stream is the one exception to "a signed request is answered
/// sealed", in every mode that accepts a signed request.
#[tokio::test]
async fn the_unsigned_stream_holds_under_optional_and_unsigned_request_sealed_reply() {
    for mode in [
        EnvelopeMode::Required,
        EnvelopeMode::Optional,
        EnvelopeMode::UnsignedRequestSealedReply,
    ] {
        for method in [Method::GET, Method::POST] {
            let hits = Hits::default();
            let layer = builder().policy(mode).build().expect("layer");
            let router = router(layer, &hits, Serve::Items);
            let (request, ..) = open(method.clone(), MARKED, &[BOTH]).await;
            let answer = send(&router, request).await;
            let at = format!("{mode:?} {method}");
            assert_eq!(answer.status, StatusCode::OK, "{at}");
            assert_eq!(answer.content_type(), CBOR_SEQ, "{at}");
            assert_eq!(answer.seen("cratestack-frames"), "unsigned", "{at}");
            assert!(!answer.is_sealed(), "{at}");
            assert_eq!(hits.get(), 1, "{at}");
        }
    }
}
