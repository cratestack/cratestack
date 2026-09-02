//! The security-review finding: `StoreErrorPolicy::Allow` is
//! class-conditional (cratestack#846).
//!
//! A store that cannot be *reached* may be served through; a store that
//! is reachable and *refusing* may not, because that failure is
//! caller-inducible — see `super::policy::StoreErrorPolicy` for the full
//! argument, and `super::store_error` for the unit-level table.
//!
//! These assert on STATUS, not on the `WARN` the layer emits alongside it.
//! An earlier revision asserted on captured log events and was flaky:
//! `tracing` caches a callsite's interest globally, so whether a
//! thread-local `set_default` subscriber sees an event depends on whether
//! some other test in the binary reached that same callsite first with no
//! subscriber installed — which the sibling `tests_store_timeout` and
//! `tests_typed_bodies` modules do. Scheduling-dependent, and a test that
//! passes for scheduling reasons is worse than no test. The decision the
//! log reports is the same boolean these tests assert through the status
//! code, and the throttle itself is covered deterministically in
//! `cratestack_core::log_throttle`.

#![cfg(test)]

use std::sync::Arc;

use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CratestackCodec, CratestackErrorResponse, RateLimitConfig};
use http::StatusCode;
use tower::{Layer as TowerLayer, Service};

use super::layer::RateLimitLayer;
use super::policy::StoreErrorPolicy;
use super::tests_support::{
    RefusingStore, UnreachableStore, authed_request, content_type_and_body, ok_service,
};

/// A store that cannot be reached is the one case the default serves
/// through: nothing a caller sent caused it, no caller can fix it, and it
/// self-heals when the socket is replaced.
#[tokio::test]
async fn an_unreachable_store_is_served_through_under_the_default_policy() {
    let layer = RateLimitLayer::new(Arc::new(UnreachableStore), RateLimitConfig::new(10, 1.0));
    let mut svc = layer.layer(ok_service());
    let status = svc.call(authed_request()).await.unwrap().status();

    assert_eq!(
        status,
        StatusCode::OK,
        "a transport-class store failure must degrade to unlimited, not to a 500 on every \
         rate-limited route"
    );
}

/// **The blocker.** The default key function hashes an *unvalidated*
/// `Authorization` header, so an unauthenticated caller mints one Redis
/// key per request; drive that to `maxmemory` and every write fails with
/// `OOM`. If `Allow` served through *that*, anyone could switch the
/// limiter off globally — including for buckets already exhausted. An
/// `OOM` is reachable, deterministic and caller-induced, so it must be
/// refused under `Allow` exactly as under `Deny`.
#[tokio::test]
async fn a_reachable_but_refusing_store_is_refused_even_under_the_default_policy() {
    let layer = RateLimitLayer::new(Arc::new(RefusingStore), RateLimitConfig::new(10, 1.0));
    let mut svc = layer.layer(ok_service());
    let response = svc.call(authed_request()).await.unwrap();

    assert_eq!(
        response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "an OOM (or any reachable-but-refusing store) must NOT open the gate: it is \
         caller-inducible, so serving through it is a global limiter bypass"
    );
    let (content_type, body) = content_type_and_body(response).await;
    assert_eq!(content_type, "application/cbor");
    let decoded: CratestackErrorResponse = CborCodec
        .decode(&body)
        .expect("the refusal body must decode as the framework error envelope");
    assert_eq!(decoded.code, "INTERNAL_ERROR");
    assert_eq!(
        decoded.message, "internal error",
        "the driver's OOM text stays operator-only"
    );
}

/// `Deny` is unconditional — even the transport-class case refuses.
#[tokio::test]
async fn deny_refuses_an_unreachable_store_with_a_decodable_typed_body() {
    let layer = RateLimitLayer::new(Arc::new(UnreachableStore), RateLimitConfig::new(10, 1.0))
        .with_store_error_policy(StoreErrorPolicy::Deny);
    let mut svc = layer.layer(ok_service());
    let response = svc.call(authed_request()).await.unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let (_, body) = content_type_and_body(response).await;
    let decoded: CratestackErrorResponse = CborCodec.decode(&body).expect(
        "the refusal body must decode as the framework error envelope — decoding it as \
         anything else is the 'unrecognized error body' bug from cratestack#846",
    );
    assert_eq!(decoded.code, "UNAVAILABLE");
}
