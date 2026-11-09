//! `SubscriptionRateLimiter` (design §6.7): the WebSocket / WebTransport
//! seam over the same store, bucket, policy and timeout as the HTTP layer.

#![cfg(test)]

use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::Request;
use cratestack_core::subscription_wire::{
    SubscriptionCharge, SubscriptionRateLimit, SubscriptionRefusal,
};
use cratestack_core::{CratestackError, RateLimitConfig, RateLimitDecision};
use http::StatusCode;
use tower::{Layer as TowerLayer, Service};

use super::layer::RateLimitLayer;
use super::policy::StoreErrorPolicy;
use super::store::{InMemoryRateLimitStore, RateLimitStore};
use super::tests_support::{RefusingStore, SlowStore, UnreachableStore, ok_service};
use super::{OpAdmission, SubscriptionRateLimiter};

/// An in-memory store that remembers every key it was asked about.
#[derive(Default)]
struct Counting {
    inner: InMemoryRateLimitStore,
    keys: Mutex<Vec<String>>,
}

#[async_trait]
impl RateLimitStore for Counting {
    async fn consume(
        &self,
        key: &str,
        config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        self.keys.lock().unwrap().push(key.to_owned());
        self.inner.consume(key, config).await
    }
}

fn limiter(store: Arc<dyn RateLimitStore>, burst: u32) -> SubscriptionRateLimiter {
    SubscriptionRateLimiter::new(store, RateLimitConfig::new(burst, 0.001))
}

async fn admit(
    limiter: &SubscriptionRateLimiter,
    op: &str,
    principal: Option<&str>,
    remote: Option<&str>,
) -> Result<(), SubscriptionRefusal> {
    let remote = remote.map(|ip| ip.parse::<IpAddr>().unwrap());
    limiter
        .admit(SubscriptionCharge::new(op, principal, remote))
        .await
}

fn code(refusal: SubscriptionRefusal) -> String {
    refusal.frame(1).err.expect("an err frame").code
}

#[tokio::test]
async fn a_principal_is_charged_under_princ_and_throttled_with_retry_after() {
    let store = Arc::new(Counting::default());
    let limiter = limiter(store.clone(), 1);
    admit(&limiter, "procedure.countdown", Some("u-1"), None)
        .await
        .unwrap();
    let refused = admit(&limiter, "procedure.countdown", Some("u-1"), None)
        .await
        .unwrap_err();
    assert!(matches!(
        refused,
        SubscriptionRefusal::Throttled { retry_after_secs } if retry_after_secs > 0
    ));
    let keys = store.keys.lock().unwrap();
    assert_eq!(keys.len(), 2);
    assert!(
        keys[0].starts_with("princ:") && keys[0] == keys[1],
        "{keys:?}"
    );
}

#[tokio::test]
async fn a_principals_bucket_is_the_http_verified_principal_bucket() {
    let store: Arc<dyn RateLimitStore> = Arc::new(InMemoryRateLimitStore::new());
    let layer = RateLimitLayer::new(store, RateLimitConfig::new(1, 0.001));
    let mut http = layer.layer(ok_service());
    let mut request = Request::builder().body(Body::empty()).unwrap();
    request
        .extensions_mut()
        .insert(super::VerifiedPrincipal("u-1".to_owned()));
    assert_eq!(http.call(request).await.unwrap().status(), StatusCode::OK);

    let refused = admit(&layer.for_subscriptions(), "x", Some("u-1"), None)
        .await
        .unwrap_err();
    assert!(matches!(refused, SubscriptionRefusal::Throttled { .. }));
}

#[tokio::test]
async fn anonymous_sessions_share_an_ipv6_slash_64() {
    let limiter = limiter(Arc::new(InMemoryRateLimitStore::new()), 1);
    admit(&limiter, "x", None, Some("2001:db8:1:2::1"))
        .await
        .unwrap();
    let refused = admit(&limiter, "x", None, Some("2001:db8:1:2:ffff::9"))
        .await
        .unwrap_err();
    assert!(matches!(refused, SubscriptionRefusal::Throttled { .. }));
    admit(&limiter, "x", None, Some("2001:db8:1:3::1"))
        .await
        .unwrap();
}

#[tokio::test]
async fn an_anonymous_session_without_a_peer_is_refused_412_without_touching_the_store() {
    let store = Arc::new(Counting::default());
    let limiter = limiter(store.clone(), 1);
    let refused = admit(&limiter, "x", None, None).await.unwrap_err();
    assert!(matches!(
        refused,
        SubscriptionRefusal::Error(CratestackError::PreconditionFailed(_))
    ));
    assert!(store.keys.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_exempt_op_derives_no_key_and_calls_no_store() {
    let store = Arc::new(Counting::default());
    let limiter = limiter(store.clone(), 1).with_op_resolver(|op| {
        if op == "free" {
            OpAdmission::new("free", true, false)
        } else {
            OpAdmission::unresolved()
        }
    });
    admit(&limiter, "free", None, None).await.unwrap();
    assert!(store.keys.lock().unwrap().is_empty());
    let refused = admit(&limiter, "other", None, None).await.unwrap_err();
    assert!(matches!(
        refused,
        SubscriptionRefusal::Error(CratestackError::PreconditionFailed(_))
    ));
}

#[tokio::test]
async fn store_failures_follow_the_layers_policy() {
    let allow = limiter(Arc::new(UnreachableStore), 1);
    admit(&allow, "x", Some("u"), None).await.unwrap();

    let deny =
        limiter(Arc::new(UnreachableStore), 1).with_store_error_policy(StoreErrorPolicy::Deny);
    let refused = admit(&deny, "x", Some("u"), None).await.unwrap_err();
    assert_eq!(code(refused), "unavailable");

    let logical = limiter(Arc::new(RefusingStore), 1);
    let refused = admit(&logical, "x", Some("u"), None).await.unwrap_err();
    assert!(matches!(refused, SubscriptionRefusal::Error(_)));
}

#[tokio::test(start_paused = true)]
async fn a_slow_store_is_bounded_by_the_timeout() {
    let store = Arc::new(SlowStore {
        delay: Duration::from_secs(1),
    });
    let limiter = limiter(store, 1).with_store_timeout(Duration::from_millis(10));
    admit(&limiter, "x", Some("u"), None).await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn for_subscriptions_copies_policy_and_timeout() {
    let store = Arc::new(SlowStore {
        delay: Duration::from_secs(1),
    });
    let layer = RateLimitLayer::new(store, RateLimitConfig::new(1, 0.001))
        .with_store_error_policy(StoreErrorPolicy::Deny)
        .with_store_timeout(Duration::from_millis(10));
    let refused = admit(&layer.for_subscriptions(), "x", Some("u"), None)
        .await
        .unwrap_err();
    assert_eq!(code(refused), "unavailable");
}
