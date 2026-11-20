//! Ending a session when the principal is revoked or the credential expires
//! (cratestack#1381): the control stream gets `{id: 0, err}` with the typed
//! reason and the session closes 4401 / 4403 / 4500. WebTransport had no
//! session-level expiry before this; it has the one WebSocket has.

mod support;

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cratestack_core::rpc::RpcFrameValue;
use cratestack_core::{
    CratestackCodec, CratestackContext, CratestackError, RevocationKey, StreamEndReason,
    StreamRevocations, SubscriptionDispatcher, SubscriptionOpen, SubscriptionStream, Value,
};
use cratestack_webtransport::{
    ClientFrame, OriginPolicy, WebTransportConfig, WebTransportServer, WireCodec, close_code,
};
use futures_util::stream;
use std::sync::atomic::{AtomicBool, Ordering};
use support::{Client, Running};

static ENTERED: AtomicBool = AtomicBool::new(false);
static GATE: AtomicBool = AtomicBool::new(false);

/// `Bearer <sub>.<sid>.<seconds the credential lives>`.
fn auth(headers: &http::HeaderMap) -> Result<CratestackContext, CratestackError> {
    let value = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| CratestackError::Unauthorized("no credential".into()))?;
    let mut parts = value.split('.');
    let (sub, sid, ttl) = (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap(),
    );
    // `gated` parks its authentication until the test opens GATE, so a
    // revoke can land mid-refresh. Blocks a worker thread: multi_thread tests.
    if sid == "gated" {
        ENTERED.store(true, Ordering::SeqCst);
        while !GATE.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    Ok(CratestackContext::authenticated([
        ("sub".to_owned(), Value::String(sub.to_owned())),
        ("sid".to_owned(), Value::String(sid.to_owned())),
        (
            "exp".to_owned(),
            Value::Int(now + ttl.parse::<i64>().unwrap()),
        ),
    ]))
}

#[derive(Clone)]
struct Dispatch(StreamRevocations);

impl SubscriptionDispatcher for Dispatch {
    async fn open<C: CratestackCodec>(
        &self,
        _open: SubscriptionOpen,
        _codec: C,
    ) -> Result<SubscriptionStream<RpcFrameValue>, CratestackError> {
        Ok(Box::pin(stream::pending()))
    }

    fn revocations(&self) -> Option<&StreamRevocations> {
        Some(&self.0)
    }
}

fn start(revocations: &StreamRevocations) -> Running {
    let (chain, key, hash) = support::cert();
    let leaf = chain[0].clone();
    let config = WebTransportConfig::new("127.0.0.1:0".parse().unwrap(), chain, key)
        .origins(OriginPolicy::Allow(vec![support::ALLOWED_ORIGIN.into()]));
    let server = WebTransportServer::bind(config, auth, Dispatch(revocations.clone())).unwrap();
    let port = server.local_addr().unwrap().port();
    tokio::spawn(server.serve());
    Running {
        url: format!("https://127.0.0.1:{port}/rpc/wt"),
        hash,
        cert: leaf,
        dispatcher: Default::default(),
    }
}

fn reason(frame: &cratestack_webtransport::ServerFrame) -> String {
    frame.err.as_ref().unwrap().details.as_ref().unwrap()["reason"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A session for `u1` / `s1` valid for `ttl` seconds, with one subscription open.
async fn session_with_a_stream(server: &Running, ttl: u64) -> (Client, support::Stream) {
    let client = server.session(Some(&format!("u1.s1.{ttl}"))).await;
    let mut stream = client
        .subscribe(1, "test.forever", serde_json::Value::Null)
        .await;
    assert!(stream.next().await.unwrap().ready.is_some());
    (client, stream)
}

#[tokio::test]
async fn a_session_end_closes_the_session_4401_with_its_reason() {
    let revocations = StreamRevocations::default();
    let server = start(&revocations);
    let (mut client, mut stream) = session_with_a_stream(&server, 3600).await;

    // The session (this dispatcher's streams register no watch of their own).
    let ended = revocations.revoke(
        RevocationKey::Session("s1".into()),
        StreamEndReason::SessionEnded,
        None,
    );
    assert_eq!(ended, 1);

    // The open stream is told why, on its own stream, before the session
    // closes: a client reads per stream and stops at the close.
    let told = stream.next().await.expect("an err frame on the stream");
    assert_eq!((told.id, reason(&told).as_str()), (1, "session_ended"));
    let frame = client.control_frame().await.expect("an {id: 0, err} frame");
    assert_eq!(frame.id, 0);
    assert_eq!(reason(&frame), "session_ended");
    assert_eq!(client.close_code().await, close_code::UNAUTHENTICATED);
}

#[tokio::test]
async fn an_expiry_tells_every_open_stream_before_the_session_closes() {
    let revocations = StreamRevocations::default();
    let server = start(&revocations);
    let (mut client, mut first) = session_with_a_stream(&server, 1).await;
    let mut second = client
        .subscribe(2, "test.forever", serde_json::Value::Null)
        .await;
    assert!(second.next().await.unwrap().ready.is_some());
    for (id, stream) in [(1, &mut first), (2, &mut second)] {
        let told = stream.next().await.expect("an err frame on the stream");
        assert_eq!(
            (told.id, reason(&told).as_str()),
            (id, "credential_expired")
        );
    }
    let frame = client.control_frame().await.expect("an {id: 0, err} frame");
    assert_eq!(reason(&frame), "credential_expired");
    assert_eq!(client.close_code().await, close_code::UNAUTHENTICATED);
}

#[tokio::test]
async fn a_suspension_closes_the_session_4403() {
    let revocations = StreamRevocations::default();
    let server = start(&revocations);
    let (mut client, _stream) = session_with_a_stream(&server, 3600).await;
    revocations.revoke(
        RevocationKey::Subject("u1".into()),
        StreamEndReason::PrincipalSuspended,
        None,
    );
    let frame = client.control_frame().await.expect("an {id: 0, err} frame");
    assert_eq!(reason(&frame), "principal_suspended");
    assert_eq!(client.close_code().await, close_code::FORBIDDEN);
}

#[tokio::test]
async fn a_tombstoned_principal_is_refused_before_ready() {
    let revocations = StreamRevocations::default();
    let server = start(&revocations);
    revocations.revoke(
        RevocationKey::Session("s1".into()),
        StreamEndReason::DeviceRevoked,
        Some(SystemTime::now() + Duration::from_secs(60)),
    );
    let session = server.connect_with(server.request()).await.unwrap();
    let mut client = Client::new(session, WireCodec::Cbor).await;
    let refused = client
        .init([("authorization".to_owned(), "Bearer u1.s1.3600".to_owned())].into())
        .await;
    assert!(refused.ready.is_none());
    assert_eq!(reason(&refused), "device_revoked");
    assert_eq!(client.close_code().await, close_code::FORBIDDEN);
}

/// New with #1381: before, a session outlived its credential's `exp` (only
/// each stream's own bound ended it).
#[tokio::test]
async fn the_session_closes_4401_when_its_credential_expires() {
    let revocations = StreamRevocations::default();
    let server = start(&revocations);
    let mut client = server.session(Some("u1.s1.1")).await;
    let frame = client.control_frame().await.expect("an {id: 0, err} frame");
    assert_eq!(reason(&frame), "credential_expired");
    assert_eq!(client.close_code().await, close_code::UNAUTHENTICATED);
}

/// A refresh before `exp` moves the session's expiry: it outlives the
/// credential it opened with.
#[tokio::test]
async fn a_refresh_before_expiry_keeps_the_session_open() {
    let revocations = StreamRevocations::default();
    let server = start(&revocations);
    let mut client = server.session(Some("u1.s1.1")).await;
    client
        .send_control(&ClientFrame::init(BTreeMap::from([(
            "authorization".to_owned(),
            "Bearer u1.s1.3600".to_owned(),
        )])))
        .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let mut stream = client
        .subscribe(2, "test.forever", serde_json::Value::Null)
        .await;
    assert!(stream.next().await.unwrap().ready.is_some(), "still open");
}

/// A refresh onto a key that was revoked meanwhile is refused.
#[tokio::test]
async fn a_refresh_onto_a_tombstoned_key_is_refused() {
    let revocations = StreamRevocations::default();
    let server = start(&revocations);
    let mut client = server.session(Some("u1.s1.3600")).await;
    revocations.revoke(
        RevocationKey::Session("s2".into()),
        StreamEndReason::DeviceRevoked,
        Some(SystemTime::now() + Duration::from_secs(60)),
    );
    client
        .send_control(&ClientFrame::init(BTreeMap::from([(
            "authorization".to_owned(),
            "Bearer u1.s2.3600".to_owned(),
        )])))
        .await;
    let frame = client.control_frame().await.expect("an {id: 0, err} frame");
    assert_eq!(reason(&frame), "device_revoked");
    assert_eq!(client.close_code().await, close_code::FORBIDDEN);
}

/// A revoke that lands while a refresh is being authenticated: the refresh
/// must not replace a watch that already ended, or the session outlives it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_revoke_during_a_refresh_still_ends_the_session() {
    let revocations = StreamRevocations::default();
    let server = start(&revocations);
    let mut client = server.session(Some("u1.s1.3600")).await;
    client
        .send_control(&ClientFrame::init(BTreeMap::from([(
            "authorization".to_owned(),
            "Bearer u1.gated.3600".to_owned(),
        )])))
        .await;
    while !ENTERED.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    revocations.revoke(
        RevocationKey::Session("s1".into()),
        StreamEndReason::SessionEnded,
        None,
    );
    GATE.store(true, Ordering::SeqCst);
    let frame = client.control_frame().await.expect("an {id: 0, err} frame");
    assert_eq!(reason(&frame), "session_ended");
    assert_eq!(client.close_code().await, close_code::UNAUTHENTICATED);
}

/// The native client, end to end: a revoke surfaces on the subscription as
/// its terminal error, typed, rather than as a bare stream that stopped.
#[cfg(feature = "client")]
#[tokio::test]
async fn the_native_client_reads_the_typed_end_of_a_revoked_subscription() {
    use cratestack_webtransport::{ClientFailure, ServerTrust, WebTransportClient};

    let revocations = StreamRevocations::default();
    let server = start(&revocations);
    let url = server.url.parse().unwrap();
    let trust = ServerTrust::CertificateHashes(vec![server.hash.clone().try_into().unwrap()]);
    let headers = [("authorization".to_owned(), "Bearer u1.s1.3600".to_owned())].into();
    let client = WebTransportClient::connect(&url, &trust, headers)
        .await
        .expect("the session opens");
    let mut stream = client
        .subscribe("test.forever", serde_json::Value::Null)
        .await
        .expect("the subscription opens");
    revocations.revoke(
        RevocationKey::Session("s1".into()),
        StreamEndReason::DeviceRevoked,
        None,
    );
    let Some(Err(ClientFailure::Refused(body))) = stream.next().await else {
        panic!("the subscription must end with the typed refusal");
    };
    assert_eq!(body.code, "permission_denied");
    assert_eq!(body.details.unwrap()["reason"], "device_revoked");
    assert!(stream.next().await.is_none());
}
