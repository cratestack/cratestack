//! The cases both transports must pass (cratestack#1381).

use std::time::{Duration, SystemTime};

use cratestack::axum::Router;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Response, StatusCode};
use cratestack::{RevocationKey, StreamEndReason, StreamRevocations};
use tower::util::ServiceExt;

use super::{CBOR_SEQ, SSE, err_of, now, open, opened};

pub fn far() -> SystemTime {
    SystemTime::now() + Duration::from_secs(3600)
}

/// `revoke` ends a live cbor-seq stream with the typed reason and code.
pub async fn session_ended_ends_a_cbor_seq_stream(
    router: &Router,
    revocations: &StreamRevocations,
    path: &str,
) {
    let live = opened(router, open(path, CBOR_SEQ, "u1", "s1", None)).await;
    let ended = revocations.revoke(
        RevocationKey::Session("s1".into()),
        StreamEndReason::SessionEnded,
        None,
    );
    assert_eq!(ended, 1);
    let (code, reason) = err_of(&live.rest().await);
    assert_eq!(
        (code.as_str(), reason.as_str()),
        ("unauthenticated", "session_ended")
    );
}

/// A suspended principal's SSE stream ends with `permission_denied`.
pub async fn suspension_ends_an_sse_stream(
    router: &Router,
    revocations: &StreamRevocations,
    path: &str,
) {
    let live = opened(router, open(path, SSE, "u2", "s2", None)).await;
    revocations.revoke(
        RevocationKey::Subject("u2".into()),
        StreamEndReason::PrincipalSuspended,
        None,
    );
    let text = String::from_utf8(live.rest().await).unwrap();
    assert!(text.contains("permission_denied"), "{text}");
    assert!(
        text.contains("\"reason\":\"principal_suspended\""),
        "{text}"
    );
}

/// Another principal's stream is left running, and a reopen with the revoked
/// principal's cached context is refused until the tombstone lapses.
pub async fn a_revoke_is_scoped_and_refuses_a_reopen(
    router: &Router,
    revocations: &StreamRevocations,
    path: &str,
) {
    let mut other = opened(router, open(path, CBOR_SEQ, "u4", "s-other", None)).await;
    let live = opened(router, open(path, CBOR_SEQ, "u3", "s3", None)).await;
    revocations.revoke(
        RevocationKey::Session("s3".into()),
        StreamEndReason::DeviceRevoked,
        Some(far()),
    );
    let (code, reason) = err_of(&live.rest().await);
    assert_eq!(
        (code.as_str(), reason.as_str()),
        ("permission_denied", "device_revoked")
    );
    assert!(
        other.stays_open().await,
        "an unrelated stream keeps running"
    );

    let reopened = router
        .clone()
        .oneshot(open(path, CBOR_SEQ, "u3", "s3", None))
        .await
        .unwrap();
    assert_eq!(reopened.status(), StatusCode::FORBIDDEN);
    assert_eq!(body_reason(reopened).await, "device_revoked");

    let fresh = router
        .clone()
        .oneshot(open(path, CBOR_SEQ, "u3", "s3-new", None))
        .await
        .unwrap();
    assert_eq!(fresh.status(), StatusCode::OK, "a new session is admitted");
}

async fn body_reason(response: Response<Body>) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    json["details"]["reason"].as_str().unwrap().to_owned()
}

/// The credential clock still ends a stream, now with a typed reason.
pub async fn an_expired_credential_ends_with_its_reason(router: &Router, path: &str) {
    let live = opened(router, open(path, CBOR_SEQ, "u5", "s5", Some(now() - 1))).await;
    let (code, reason) = err_of(&live.rest().await);
    assert_eq!(
        (code.as_str(), reason.as_str()),
        ("unauthenticated", "credential_expired")
    );
}
