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
use support::{Client, Running};

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
    let (mut client, _stream) = session_with_a_stream(&server, 3600).await;

    // The session (this dispatcher's streams register no watch of their own).
    let ended = revocations.revoke(
        RevocationKey::Session("s1".into()),
        StreamEndReason::SessionEnded,
        None,
    );
    assert_eq!(ended, 1);

    let frame = client.control_frame().await.expect("an {id: 0, err} frame");
    assert_eq!(frame.id, 0);
    assert_eq!(reason(&frame), "session_ended");
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
