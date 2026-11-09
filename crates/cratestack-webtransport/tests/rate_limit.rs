//! Subscription streams are charged one token each (design §6.7), against a
//! scripted limiter so the binding is tested without axum's rate limiter.

mod support;

use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cratestack_webtransport::{
    OriginPolicy, SubscriptionCharge, SubscriptionRateLimit, SubscriptionRefusal,
    WebTransportConfig, WebTransportLimits, WebTransportServer,
};
use support::{ALLOWED_ORIGIN, Dispatcher, Running, auth, cert};

type Seen = Vec<(String, Option<String>, Option<IpAddr>)>;

/// Admits while `allow > 0`, then throttles for 5s; records every charge.
#[derive(Debug, Clone, Default)]
struct Budget {
    allow: Arc<AtomicUsize>,
    seen: Arc<Mutex<Seen>>,
}

impl Budget {
    fn allowing(n: usize) -> Self {
        let budget = Self::default();
        budget.allow.store(n, Ordering::SeqCst);
        budget
    }
}

#[async_trait]
impl SubscriptionRateLimit for Budget {
    async fn admit(&self, charge: SubscriptionCharge<'_>) -> Result<(), SubscriptionRefusal> {
        self.seen.lock().unwrap().push((
            charge.op_id.to_owned(),
            charge.principal.map(str::to_owned),
            charge.remote,
        ));
        let left = self.allow.load(Ordering::SeqCst);
        if left == 0 {
            return Err(SubscriptionRefusal::Throttled {
                retry_after_secs: 5,
            });
        }
        self.allow.store(left - 1, Ordering::SeqCst);
        Ok(())
    }
}

fn start_limited(limits: WebTransportLimits, limiter: Budget) -> Running {
    let (chain, key, hash) = cert();
    let leaf = chain[0].clone();
    let config = WebTransportConfig::new("127.0.0.1:0".parse().unwrap(), chain, key)
        .origins(OriginPolicy::Allow(vec![ALLOWED_ORIGIN.into()]))
        .limits(limits)
        .rate_limit(limiter);
    let dispatcher = Dispatcher::default();
    let server = WebTransportServer::bind(config, auth, dispatcher.clone()).unwrap();
    let port = server.local_addr().unwrap().port();
    tokio::spawn(server.serve());
    Running {
        url: format!("https://127.0.0.1:{port}/rpc/wt"),
        hash,
        cert: leaf,
        dispatcher,
    }
}

#[tokio::test]
async fn a_throttled_stream_is_refused_before_ready_and_the_session_stays() {
    let budget = Budget::allowing(1);
    let server = start_limited(WebTransportLimits::default(), budget.clone());
    let client = server.session(Some("7")).await;

    let mut first = client
        .subscribe(1, "test.items", serde_json::json!({ "n": 1 }))
        .await;
    assert!(first.next().await.unwrap().ready.is_some());
    assert!(first.next().await.unwrap().next.is_some());
    assert!(first.next().await.unwrap().end.is_some());

    let mut refused = client
        .subscribe(2, "test.items", serde_json::json!({ "n": 1 }))
        .await;
    let frame = refused.next().await.unwrap();
    assert!(frame.ready.is_none());
    let err = frame.err.expect("an err frame");
    assert_eq!(err.code, "resource_exhausted");
    assert_eq!(
        err.details,
        Some(serde_json::json!({ "retry_after_secs": 5 }))
    );
    assert!(refused.next().await.is_none(), "FIN after the refusal");

    budget.allow.store(1, Ordering::SeqCst);
    let mut third = client
        .subscribe(3, "test.items", serde_json::json!({ "n": 1 }))
        .await;
    assert!(third.next().await.unwrap().ready.is_some());
}

#[tokio::test]
async fn the_limiter_sees_principal_and_quic_peer() {
    let budget = Budget::allowing(2);
    let server = start_limited(WebTransportLimits::default(), budget.clone());
    let peer = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));

    let signed_in = server.session(Some("7")).await;
    let mut stream = signed_in
        .subscribe(1, "test.items", serde_json::json!({ "n": 0 }))
        .await;
    assert!(stream.next().await.unwrap().ready.is_some());
    let anonymous = server.session(None).await;
    let mut stream = anonymous
        .subscribe(1, "test.items", serde_json::json!({ "n": 0 }))
        .await;
    assert!(stream.next().await.unwrap().ready.is_some());

    let seen = budget.seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![
            ("test.items".to_owned(), Some("7".to_owned()), peer),
            ("test.items".to_owned(), None, peer),
        ]
    );
}

#[tokio::test]
async fn a_throttled_stream_releases_its_permit() {
    let budget = Budget::allowing(0);
    let limits = WebTransportLimits {
        subscriptions_per_session: 1,
        ..WebTransportLimits::default()
    };
    let server = start_limited(limits, budget.clone());
    let client = server.session(Some("7")).await;

    let mut first = client
        .subscribe(1, "test.forever", serde_json::Value::Null)
        .await;
    let err = first.next().await.unwrap().err.expect("throttled");
    assert_eq!(
        err.details,
        Some(serde_json::json!({ "retry_after_secs": 5 }))
    );

    budget.allow.store(1, Ordering::SeqCst);
    let mut second = client
        .subscribe(2, "test.forever", serde_json::Value::Null)
        .await;
    assert!(second.next().await.unwrap().ready.is_some());
}
