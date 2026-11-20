//! Ending a socket and its subscriptions when the principal is revoked
//! (cratestack#1381), over a real socket: `{id: 0, err}` with the typed
//! reason, then 4401 / 4403 / 4500; a tombstoned principal is refused before
//! `Ready`; and a refresh (`Init`) does not extend streams already open.

mod revocation_support;

use std::time::{Duration, SystemTime};

use cratestack_core::{RevocationKey, StreamEndReason, StreamRevocations};
use cratestack_websocket::{ClientFrame, close_code};
use futures_util::SinkExt;
use revocation_support::{
    ENTERED, GATE, Next, connect, credentials, encode, frame, next, reason, serve, session,
    until_closed,
};

#[tokio::test]
async fn a_session_end_closes_the_socket_4401_and_every_open_subscription() {
    let revocations = StreamRevocations::default();
    let url = serve(&revocations).await;
    let mut socket = session(&url, "long", 2).await;

    let ended = revocations.revoke(
        RevocationKey::Session("s1".into()),
        StreamEndReason::SessionEnded,
        None,
    );
    assert_eq!(ended, 3, "the session and its two subscriptions");

    let (code, session_reason, stream_reasons) = until_closed(&mut socket).await;
    assert_eq!(code, close_code::UNAUTHENTICATED as u16);
    assert_eq!(session_reason, "session_ended");
    // Every open stream is told why, in order, before the session frame.
    let expected = [
        (1, "session_ended".to_owned()),
        (2, "session_ended".to_owned()),
    ];
    assert_eq!(stream_reasons, expected);
}

#[tokio::test]
async fn a_credential_expiry_tells_every_open_stream_before_closing() {
    let revocations = StreamRevocations::default();
    let url = serve(&revocations).await;
    let mut socket = session(&url, "short", 2).await;
    let (code, session_reason, stream_reasons) = until_closed(&mut socket).await;
    assert_eq!(code, close_code::UNAUTHENTICATED as u16);
    assert_eq!(session_reason, "credential_expired");
    let ids: Vec<u64> = stream_reasons.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, [1, 2]);
    assert!(
        stream_reasons
            .iter()
            .all(|(_, reason)| reason == "credential_expired")
    );
}

/// A refresh onto a tombstoned key is refused (a key revoked meanwhile).
#[tokio::test]
async fn an_init_refresh_onto_a_tombstoned_key_is_refused() {
    let revocations = StreamRevocations::default();
    let url = serve(&revocations).await;
    let mut socket = session(&url, "long", 1).await;
    // `rotated` is the same user on session `s2`, which was revoked.
    revocations.revoke(
        RevocationKey::Session("s2".into()),
        StreamEndReason::DeviceRevoked,
        Some(SystemTime::now() + Duration::from_secs(60)),
    );
    socket
        .send(encode(&ClientFrame::init(credentials("rotated"))))
        .await
        .unwrap();
    let (code, session_reason, _) = until_closed(&mut socket).await;
    assert_eq!(code, close_code::FORBIDDEN as u16);
    assert_eq!(session_reason, "device_revoked");
}

/// A revoke that lands while a refresh is being authenticated: the refresh
/// must not replace a watch that already ended, or the socket outlives it.
#[tokio::test]
async fn a_revoke_during_a_refresh_still_ends_the_session() {
    let revocations = StreamRevocations::default();
    let url = serve(&revocations).await;
    let mut socket = session(&url, "long", 1).await;
    socket
        .send(encode(&ClientFrame::init(credentials("gated"))))
        .await
        .unwrap();
    ENTERED.notified().await;
    revocations.revoke(
        RevocationKey::Session("s1".into()),
        StreamEndReason::SessionEnded,
        None,
    );
    GATE.notify_one();
    let (code, session_reason, _) = until_closed(&mut socket).await;
    assert_eq!(code, close_code::UNAUTHENTICATED as u16);
    assert_eq!(session_reason, "session_ended");
}

#[tokio::test]
async fn a_suspension_closes_the_socket_4403() {
    let revocations = StreamRevocations::default();
    let url = serve(&revocations).await;
    let mut socket = session(&url, "long", 1).await;
    revocations.revoke(
        RevocationKey::Subject("u1".into()),
        StreamEndReason::PrincipalSuspended,
        None,
    );
    let (code, session_reason, _) = until_closed(&mut socket).await;
    assert_eq!(code, close_code::FORBIDDEN as u16);
    assert_eq!(session_reason, "principal_suspended");
}

#[tokio::test]
async fn a_lost_revocation_feed_closes_the_socket_4500_so_it_is_retried_not_signed_out() {
    let revocations = StreamRevocations::default();
    let url = serve(&revocations).await;
    let mut socket = session(&url, "long", 1).await;
    revocations.end_all(StreamEndReason::RevocationResync);
    let (code, session_reason, _) = until_closed(&mut socket).await;
    assert_eq!(code, close_code::SERVER_ERROR as u16);
    assert_eq!(session_reason, "revocation_resync");
}

#[tokio::test]
async fn a_tombstoned_principal_is_refused_before_ready() {
    let revocations = StreamRevocations::default();
    let url = serve(&revocations).await;
    revocations.revoke(
        RevocationKey::Session("s1".into()),
        StreamEndReason::DeviceRevoked,
        Some(SystemTime::now() + Duration::from_secs(60)),
    );
    let mut socket = connect(&url).await;
    socket
        .send(encode(&ClientFrame::init(credentials("long"))))
        .await
        .unwrap();
    let refused = frame(&mut socket).await;
    assert!(refused.ready.is_none());
    assert_eq!(
        (refused.id, reason(&refused).as_str()),
        (0, "device_revoked")
    );
    assert!(
        matches!(next(&mut socket).await, Next::Closed(code) if code == close_code::FORBIDDEN as u16)
    );
}

/// A refresh re-authenticates the session; it does not stretch the lifetime
/// of a subscription that is already open (design §6.3): that one still ends
/// at the `exp` it opened with, and the session carries on.
#[tokio::test]
async fn an_init_refresh_does_not_extend_streams_already_open() {
    let revocations = StreamRevocations::default();
    let url = serve(&revocations).await;
    let mut socket = session(&url, "short", 1).await;
    socket
        .send(encode(&ClientFrame::init(credentials("long"))))
        .await
        .unwrap();

    let ended = frame(&mut socket).await;
    assert_eq!(ended.id, 1);
    assert_eq!(reason(&ended), "credential_expired");

    // The socket is alive on the refreshed credential: a new stream opens.
    socket
        .send(encode(&ClientFrame::request(
            2,
            "forever",
            serde_json::Value::Null,
        )))
        .await
        .unwrap();
    let reopened = frame(&mut socket).await;
    assert!(reopened.id == 2 && reopened.ready.is_some(), "{reopened:?}");
}
