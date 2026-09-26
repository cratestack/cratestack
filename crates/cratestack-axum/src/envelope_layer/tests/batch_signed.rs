//! A signed `/rpc/batch` (decision B1, second-review items S-2 and the
//! strictest-with-unresolved nit): before it is opened its frames are
//! unreadable, so it is opened when the strictest of `batch` and the
//! policy's `unresolved_mode` is not `Off`; once opened, a batch whose
//! every answer is `Off` is refused with the unsigned `415`, as a signed
//! unary call to an `Off` op is.

use bytes::Bytes;
use cratestack_core::request_digest;
use http::{Method, StatusCode};

use super::fixtures::{Hits, rpc_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode, EnvelopePolicy, PolicyRequest};

/// `batch` itself answers `batch`, every frame `frames`, and unresolved
/// traffic `unresolved`.
struct Modes {
    batch: EnvelopeMode,
    frames: EnvelopeMode,
    unresolved: EnvelopeMode,
}

impl EnvelopePolicy for Modes {
    fn mode(&self, request: &PolicyRequest<'_>) -> EnvelopeMode {
        if request.is_batch_frame() {
            self.frames
        } else {
            self.batch
        }
    }
    fn unresolved_mode(&self) -> EnvelopeMode {
        self.unresolved
    }
}

fn router(policy: Modes, hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(policy)
        .rpc("")
        .build()
        .expect("layer");
    rpc_router(layer, hits)
}

/// S-2: every answer `Off` (the default `unresolved_mode`, `Required`,
/// gets it opened): refused after verification with the unsigned `415`,
/// and the handler never runs.
#[tokio::test]
async fn a_signed_batch_whose_every_answer_is_off_is_the_unsigned_415() {
    let hits = Hits::default();
    let off = |_: &PolicyRequest<'_>| EnvelopeMode::Off;
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(off)
        .rpc("")
        .build()
        .expect("layer");
    let router = rpc_router(layer, &hits);
    let call = Call::new(Method::POST, "batch", &[]);
    let sealed = call.seal(&batch_frames(&["procedure.read"])).await;
    let answer = send(
        &router,
        cose_request(Method::POST, "/rpc/batch", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(!answer.is_sealed());
    assert_eq!(error_code(&answer.body), "invalid_argument");
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect_err("unsigned: nothing a client could take for its batch");
    assert_eq!(hits.get(), 0, "the handler must not run");
}

/// Before opening, the strictest of `batch` and `unresolved_mode`: `Off`
/// and `Off` refuse the body unopened (`415`, whatever it holds); any
/// other pair opens it, so a body that does not verify is the `401`.
#[tokio::test]
async fn a_signed_batch_is_opened_under_the_strictest_of_batch_and_unresolved() {
    use EnvelopeMode::{Off, Optional, Required};
    for (batch, unresolved, expected) in [
        (Off, Off, StatusCode::UNSUPPORTED_MEDIA_TYPE),
        (Off, Optional, StatusCode::UNAUTHORIZED),
        (Off, Required, StatusCode::UNAUTHORIZED),
        (Optional, Off, StatusCode::UNAUTHORIZED),
    ] {
        let hits = Hits::default();
        let policy = Modes {
            batch,
            frames: Required,
            unresolved,
        };
        let forged = Bytes::from_static(PAYLOAD);
        let answer = send(
            &router(policy, &hits),
            cose_request(Method::POST, "/rpc/batch", forged),
        )
        .await;
        assert_eq!(
            answer.status, expected,
            "batch {batch:?}, unresolved {unresolved:?}"
        );
        assert!(!answer.is_sealed());
        assert_eq!(hits.get(), 0);
    }
    // And a batch that does verify runs, sealed, under its frames' mode.
    let hits = Hits::default();
    let policy = Modes {
        batch: Off,
        frames: Required,
        unresolved: Optional,
    };
    let call = Call::new(Method::POST, "batch", &[]);
    let sealed = call.seal(&batch_frames(&["procedure.transfer"])).await;
    let answer = send(
        &router(policy, &hits),
        cose_request(Method::POST, "/rpc/batch", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("sealed for the batch");
    assert_eq!(hits.get(), 1);
}
