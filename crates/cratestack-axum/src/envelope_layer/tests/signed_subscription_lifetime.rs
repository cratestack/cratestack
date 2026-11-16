//! A signed subscription's stream ends at `max_signed_stream_life`
//! (cratestack#1378), with `unavailable` / `"stream lifetime"`, unless the
//! credential's own `exp` comes first.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::Request;
use axum::routing::get;
use cratestack_core::SubscriptionStream;
use futures_util::stream;
use http::{Method, StatusCode};

use super::fixtures::Hits;
use super::signed_subscription_support::*;
use super::support::*;
use crate::envelope_layer::{DEFAULT_MAX_SIGNED_STREAM_LIFE, MAX_SIGNED_STREAM_LIFE};
use crate::rpc::{negotiate_subscribe_binding, serve_subscription_with, until_credential_expires};

/// `send`, but a stream that never ends fails the test (a day of paused time)
/// instead of hanging it.
async fn ends(router: &Router, request: Request) -> Answer {
    tokio::time::timeout(Duration::from_secs(86_400), send(router, request))
        .await
        .expect("the stream ended on its own")
}

fn contains(body: &[u8], needle: &str) -> bool {
    body.windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[tokio::test(start_paused = true)]
async fn a_quiet_stream_ends_at_the_configured_lifetime() {
    let hits = Hits::default();
    let layer = builder()
        .max_signed_stream_life(life(30))
        .build()
        .expect("layer");
    let router = router(layer, &hits, Serve::Quiet);
    let (request, ..) = open(Method::GET, MARKED, &[CBOR_SEQ]).await;
    let answer = ends(&router, request).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.seen("x-seen-deadline-ms"), "30000");
    assert!(contains(&answer.body, "unavailable"), "{:?}", answer.body);
    assert!(
        contains(&answer.body, "stream lifetime"),
        "{:?}",
        answer.body
    );
}

#[tokio::test(start_paused = true)]
async fn the_default_lifetime_is_one_hour() {
    assert_eq!(DEFAULT_MAX_SIGNED_STREAM_LIFE, Duration::from_secs(3600));
    let layer = builder().build().expect("layer");
    let router = router(layer, &Hits::default(), Serve::Quiet);
    let (request, ..) = open(Method::GET, MARKED, &[CBOR_SEQ]).await;
    let answer = ends(&router, request).await;
    assert_eq!(answer.seen("x-seen-deadline-ms"), "3600000");
}

#[test]
fn a_zero_lifetime_is_refused_at_build() {
    let error = builder()
        .max_signed_stream_life(Duration::ZERO)
        .build()
        .expect_err("zero");
    assert!(error.to_string().contains("lifetime"), "{error}");
}

/// A lifetime above the cap is refused at build, naming the cap, so a value
/// the clock cannot hold is never configured; exactly the cap builds.
#[test]
fn a_lifetime_over_24_hours_is_refused_at_build() {
    assert_eq!(MAX_SIGNED_STREAM_LIFE, Duration::from_secs(24 * 3600));
    for over in [
        MAX_SIGNED_STREAM_LIFE + Duration::from_nanos(1),
        Duration::MAX,
    ] {
        let error = builder()
            .max_signed_stream_life(over)
            .build()
            .expect_err("over the cap");
        assert!(matches!(
            error,
            cratestack_core::CratestackError::Validation(_)
        ));
        assert!(error.to_string().contains("24 hours"), "{error}");
        assert!(
            error.to_string().contains("MAX_SIGNED_STREAM_LIFE"),
            "{error}"
        );
    }
    builder()
        .max_signed_stream_life(MAX_SIGNED_STREAM_LIFE)
        .build()
        .expect("exactly the cap builds");
}

/// The credential's `exp`, applied by the generated feed inside, is earlier
/// than the layer's deadline here, so its error ends the stream.
#[tokio::test(start_paused = true)]
async fn the_credential_expiry_wins_when_it_comes_first() {
    let layer = builder()
        .max_signed_stream_life(life(3600))
        .build()
        .expect("layer");
    let handler = |req: Request| async move {
        let binding = negotiate_subscribe_binding(req.headers()).expect("binding");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let quiet: SubscriptionStream<serde_json::Value> = Box::pin(stream::pending());
        let items = until_credential_expires(quiet, Some(now + 5));
        serve_subscription_with(binding, req.extensions(), async { Ok(items) }).await
    };
    let router = axum::Router::new()
        .route("/rpc/subscribe/{op_id}", get(handler))
        .layer(layer);
    let (request, ..) = open(Method::GET, MARKED, &[CBOR_SEQ]).await;
    let answer = ends(&router, request).await;
    assert!(
        contains(&answer.body, "credential expired"),
        "{:?}",
        answer.body
    );
    assert!(!contains(&answer.body, "stream lifetime"));
}
