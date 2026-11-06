//! cratestack#1261: what an `Idempotency-Key` does with a refusal, shared by
//! `authn_idempotency_rest.rs` and `authn_idempotency_rpc.rs` so both
//! transports run the same scenarios through the real generated router.
//!
//! The namespace is the caller's `x-sub`, as a device-key principal would
//! be: the same caller before and after renewing its credential or stepping
//! up, so a retry under the same key lands on the same reservation.

#![allow(dead_code)]

#[allow(dead_code)] // Only `MemoryIdempotency` is used here.
#[path = "../cose_support/stores.rs"]
mod stores;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cratestack::axum::Router;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{AuthRefusal, CratestackContext, CratestackError, Value};
use cratestack_axum::idempotency::IdempotencyLayer;
use tower::ServiceExt;

use crate::authn_support::{HeaderProvider, STALE_WEAK, STRONG};
use stores::MemoryIdempotency;

/// How many times a procedure body ran.
#[derive(Clone, Default)]
pub struct Runs(Arc<AtomicUsize>);

impl Runs {
    pub fn bump(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

/// `HeaderProvider`, except that `x-expired` refuses the credential the way
/// a provider does when it has run out: a 401 the client renews and retries.
#[derive(Clone)]
pub struct Provider;

impl cratestack::AuthProvider for Provider {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &cratestack::RequestContext<'_>,
    ) -> Result<CratestackContext, Self::Error> {
        if request.headers.contains_key("x-expired") {
            return Err(AuthRefusal::unauthenticated("credential expired")
                .with_member("error", Value::String("credential_expired".into()))
                .into_error());
        }
        HeaderProvider.authenticate(request).await
    }
}

/// What `refund`'s body answers after its write: a refusal it propagates
/// from a nested generated call, so already claimed by that call's dispatch
/// path. The worst case: the body's `invoke_with_db` must still disown it,
/// or the key would be released after a write.
pub fn refusal_after_write() -> CratestackError {
    AuthRefusal::unauthenticated("signing key rotated")
        .with_member("error", Value::String("key_rotated".into()))
        .into_error()
        .__generated_claim_auth_refusal()
}

/// The generated router behind an `IdempotencyLayer` keyed by `x-sub`.
pub fn keyed(router: Router) -> Router {
    router.layer(
        IdempotencyLayer::new(
            Arc::new(MemoryIdempotency::default()),
            Duration::from_secs(60),
        )
        .with_principal_fingerprint(|request| {
            request
                .headers()
                .get("x-sub")
                .and_then(|sub| sub.to_str().ok())
                .unwrap_or("anonymous")
                .to_owned()
        }),
    )
}

/// One transport's router and how it names a procedure.
pub struct Transport {
    pub app: Router,
    pub path: fn(&str) -> String,
}

/// An answer: its status, whether the layer replayed it, and its body.
pub struct Answer {
    pub status: StatusCode,
    pub replayed: bool,
    pub body: serde_json::Value,
}

impl Transport {
    pub async fn call(&self, procedure: &str, caller: &[(&str, &str)], key: &str) -> Answer {
        let mut request = Request::post((self.path)(procedure))
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .header("idempotency-key", key);
        for (name, value) in caller {
            request = request.header(*name, *value);
        }
        let request = request
            .body(Body::from(r#"{"args":{"amount":5}}"#))
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let replayed = response.headers().contains_key("idempotency-replayed");
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        Answer {
            status,
            replayed,
            body: serde_json::from_slice(&bytes).unwrap(),
        }
    }
}

/// The step-up refusal (ADR 0022) comes before the body, so it is not
/// recorded: the same key, sent again by the caller who stepped up, runs
/// `pay` once, and that success is what a third send replays.
pub async fn a_step_up_refusal_is_not_recorded(transport: Transport, runs: &Runs) {
    let refused = transport.call("pay", STALE_WEAK, "pay-1").await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);
    assert_eq!(refused.body["error"], "insufficient_user_authentication");
    assert!(!refused.replayed);

    let served = transport.call("pay", STRONG, "pay-1").await;
    assert_eq!(served.status, StatusCode::OK, "{}", served.body);
    assert!(!served.replayed, "the refusal must not have been recorded");
    assert_eq!(runs.get(), 1);

    let again = transport.call("pay", STRONG, "pay-1").await;
    assert_eq!(again.status, StatusCode::OK);
    assert!(again.replayed, "the success is recorded and replayed");
    assert_eq!(runs.get(), 1, "the key still runs `pay` exactly once");
}

/// `AuthProvider::authenticate`'s refusal (an expired credential) comes
/// before the body too: after renewal the same key runs `pay` once.
pub async fn an_authenticate_refusal_is_not_recorded(transport: Transport, runs: &Runs) {
    let expired: Vec<(&str, &str)> = STRONG.iter().copied().chain([("x-expired", "1")]).collect();
    let refused = transport.call("pay", &expired, "pay-2").await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);
    assert_eq!(refused.body["error"], "credential_expired");

    let served = transport.call("pay", STRONG, "pay-2").await;
    assert_eq!(served.status, StatusCode::OK, "{}", served.body);
    assert!(!served.replayed);
    assert_eq!(runs.get(), 1);
    assert!(transport.call("pay", STRONG, "pay-2").await.replayed);
    assert_eq!(runs.get(), 1);
}

/// `@allow`'s refusal (403) comes before the body: a caller granted the
/// role sends the same key again and `close` runs once.
pub async fn a_policy_refusal_is_not_recorded(transport: Transport, runs: &Runs) {
    let refused = transport.call("close", STRONG, "close-1").await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);

    let admin: Vec<(&str, &str)> = STRONG
        .iter()
        .copied()
        .chain([("x-roles", "admin")])
        .collect();
    let served = transport.call("close", &admin, "close-1").await;
    assert_eq!(served.status, StatusCode::OK, "{}", served.body);
    assert!(!served.replayed);
    assert_eq!(runs.get(), 1);
}

/// A refusal `refund`'s body returns after its write is the body's: it is
/// recorded and replayed, even to a caller with a stronger credential, so
/// the key never runs the write twice (GHSA-r67q-4qqq-g9gm's rule).
pub async fn a_refusal_from_the_body_is_recorded(transport: Transport, runs: &Runs) {
    let refused = transport.call("refund", STALE_WEAK, "refund-1").await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);
    assert_eq!(refused.body["error"], "key_rotated");
    assert_eq!(runs.get(), 1, "the body ran and wrote");

    let again = transport.call("refund", STRONG, "refund-1").await;
    assert_eq!(again.status, StatusCode::UNAUTHORIZED, "{}", again.body);
    assert!(again.replayed, "a body's refusal is recorded");
    assert_eq!(again.body["error"], "key_rotated");
    assert_eq!(runs.get(), 1, "the write is never run twice");
}

/// The `ProcedureRegistry` of both fixtures: `pay` and `close` count a run
/// and succeed, `refund` counts its write and then refuses.
#[macro_export]
macro_rules! registry {
    () => {
        #[derive(Clone, Default)]
        struct Procedures(authn_idempotency_support::Runs);

        impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
            async fn pay(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &cratestack::CratestackContext,
                _args: cratestack_schema::procedures::pay::Args,
                _authorized: cratestack_schema::procedures::pay::Authorized,
            ) -> Result<cratestack_schema::Receipt, cratestack::CratestackError> {
                self.0.bump();
                Ok(cratestack_schema::Receipt { ok: true })
            }

            async fn close(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &cratestack::CratestackContext,
                _args: cratestack_schema::procedures::close::Args,
                _authorized: cratestack_schema::procedures::close::Authorized,
            ) -> Result<cratestack_schema::Receipt, cratestack::CratestackError> {
                self.0.bump();
                Ok(cratestack_schema::Receipt { ok: true })
            }

            async fn refund(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &cratestack::CratestackContext,
                _args: cratestack_schema::procedures::refund::Args,
                _authorized: cratestack_schema::procedures::refund::Authorized,
            ) -> Result<cratestack_schema::Receipt, cratestack::CratestackError> {
                self.0.bump();
                Err(authn_idempotency_support::refusal_after_write())
            }
        }
    };
}
