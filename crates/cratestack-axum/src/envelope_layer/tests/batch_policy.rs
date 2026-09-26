//! Decision B1: a `/rpc/batch` call runs under the strictest mode of
//! `batch` and of every frame's op, read from the payload (opened or
//! plain), and a payload the layer cannot read is refused under any mode
//! but `Off`. A subscription is shown to the policy as its bare op id.

use std::sync::{Arc, Mutex};

use cratestack_core::request_digest;
use http::{Method, StatusCode};

use super::fixtures::{Hits, rpc_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode, PolicyRequest};

type Seen = Arc<Mutex<Vec<(String, bool, bool)>>>;

/// `procedure.transfer` is `Required`, everything else `Optional`;
/// `unresolved` is the policy's `unresolved_mode`.
fn router(hits: &Hits, seen: &Seen, unresolved: EnvelopeMode) -> axum::Router {
    struct PerOp(Seen, EnvelopeMode);
    impl crate::envelope_layer::EnvelopePolicy for PerOp {
        fn mode(&self, request: &PolicyRequest<'_>) -> EnvelopeMode {
            let entry = (
                request.op().to_owned(),
                request.is_subscription(),
                request.is_batch_frame(),
            );
            self.0.lock().expect("lock").push(entry);
            match request.op() {
                "procedure.transfer" => EnvelopeMode::Required,
                _ => EnvelopeMode::Optional,
            }
        }
        fn unresolved_mode(&self) -> EnvelopeMode {
            self.1
        }
    }
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(PerOp(seen.clone(), unresolved))
        .rpc("")
        .build()
        .expect("layer");
    rpc_router(layer, hits)
}

fn plain_batch(body: Vec<u8>) -> axum::extract::Request {
    http::Request::post("/rpc/batch")
        .header(http::header::CONTENT_TYPE, "application/cbor")
        .body(axum::body::Body::from(body))
        .expect("request")
}

#[tokio::test]
async fn a_plain_batch_cannot_carry_a_required_op() {
    let (hits, seen) = (Hits::default(), Seen::default());
    let router = router(&hits, &seen, EnvelopeMode::Optional);
    let body = batch_frames(&["procedure.read", "procedure.transfer"]);
    let answer = send(&router, plain_batch(body)).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(hits.get(), 0);
    let frames: Vec<_> = seen.lock().expect("lock").clone();
    assert!(
        frames.contains(&("procedure.transfer".to_owned(), false, true)),
        "{frames:?}"
    );
    // Only Optional ops: it runs, as a unary call to them would.
    let answer = send(&router, plain_batch(batch_frames(&["procedure.read"]))).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(hits.get(), 1);
}

#[tokio::test]
async fn an_unreadable_plain_batch_is_refused_unless_everything_is_off() {
    let hits = Hits::default();
    for (unresolved, expected) in [
        (EnvelopeMode::Required, StatusCode::UNAUTHORIZED),
        (EnvelopeMode::Optional, StatusCode::BAD_REQUEST),
    ] {
        let router = router(&hits, &Seen::default(), unresolved);
        let answer = send(&router, plain_batch(PAYLOAD.to_vec())).await;
        assert_eq!(answer.status, expected, "{unresolved:?}");
    }
    assert_eq!(hits.get(), 0);
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Off)
        .rpc("")
        .build()
        .expect("layer");
    let off = send(&rpc_router(layer, &hits), plain_batch(PAYLOAD.to_vec())).await;
    assert_eq!(off.status, StatusCode::OK, "Off: the layer is inert");
}

#[tokio::test]
async fn a_signed_batch_has_its_frames_checked_once_opened() {
    let (hits, seen) = (Hits::default(), Seen::default());
    let router = router(&hits, &seen, EnvelopeMode::Optional);
    let call = Call::new(Method::POST, "batch", &[]);
    let sealed = call.seal(&batch_frames(&["procedure.transfer"])).await;
    let answer = send(
        &router,
        cose_request(Method::POST, "/rpc/batch", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
    assert!(
        seen.lock()
            .expect("lock")
            .contains(&("procedure.transfer".to_owned(), false, true))
    );
    // Signed, but not a frame array: a sealed 400, the handler never runs.
    let sealed = call.seal(PAYLOAD).await;
    let answer = send(
        &router,
        cose_request(Method::POST, "/rpc/batch", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("sealed");
    assert_eq!(hits.get(), 1);
}

#[tokio::test]
async fn a_subscription_is_shown_to_the_policy_as_its_bare_op_id() {
    let (hits, seen) = (Hits::default(), Seen::default());
    let router = router(&hits, &seen, EnvelopeMode::Optional);
    let _ = send(
        &router,
        plain_request(Method::GET, "/rpc/subscribe/model.Widget.subscribe", b""),
    )
    .await;
    let seen = seen.lock().expect("lock").clone();
    assert_eq!(
        seen,
        vec![("model.Widget.subscribe".to_owned(), true, false)]
    );
}

/// Security finding SF-1 (second review): the generated batch handler
/// reads the frames under any spelling of the CBOR media type (case,
/// parameters), so the layer must too. It used to match the exact string,
/// found no frames under `application/cbor; charset=binary`, and with an
/// `unresolved_mode` of `Off` forwarded a batch carrying a `Required` op.
#[tokio::test]
async fn a_batch_content_type_with_parameters_or_another_case_is_still_read() {
    struct TransferOnly;
    impl crate::envelope_layer::EnvelopePolicy for TransferOnly {
        fn mode(&self, request: &PolicyRequest<'_>) -> EnvelopeMode {
            match request.op() {
                "procedure.transfer" => EnvelopeMode::Required,
                _ => EnvelopeMode::Off,
            }
        }
        fn unresolved_mode(&self) -> EnvelopeMode {
            EnvelopeMode::Off
        }
    }
    for content_type in [
        "application/cbor; charset=binary",
        "application/cbor;x=1",
        "Application/CBOR",
        "application/cbor ",
    ] {
        let hits = Hits::default();
        let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
            .policy(TransferOnly)
            .rpc("")
            .build()
            .expect("layer");
        let request = http::Request::post("/rpc/batch")
            .header(http::header::CONTENT_TYPE, content_type)
            .body(axum::body::Body::from(batch_frames(&["procedure.transfer"])))
            .expect("request");
        let answer = send(&rpc_router(layer, &hits), request).await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{content_type:?}");
        assert_eq!(hits.get(), 0, "{content_type:?}: the handler must not run");
    }
}
