//! cratestack#416: end-to-end proof, through a real macro-generated router
//! (not just the pure-function/unit coverage in `cratestack-axum`'s own test
//! suite), that the **default** `IdempotencyLayer`/`RateLimitLayer`
//! configuration cannot place two distinct unauthenticated callers in a
//! shared idempotency/rate-limit namespace — the acceptance criteria #416
//! left unmet when it was closed by #526 (which fixed #415's separate
//! `Forwarded`/`X-Forwarded-For` spoofing hole but left this one, per the
//! maintainer's own closing comment on #415: "criterion 3 is satisfied only
//! at the unit level, not end to end through the framework's real wiring").
//!
//! `cratestack-axum` cannot itself invoke `include_server_schema!` (no
//! dependency on `cratestack-macros`), so this macro-integration coverage
//! has to live here — mirroring `no_database_procedures.rs` and
//! `trusted_proxy_client_ip.rs`'s existing pattern of doing exactly that.
//! No Postgres involved: `db = None` plus `RateLimitLayer`'s in-memory
//! store, matching this crate's whole reason for existing.
//!
//! # What's actually under test
//!
//! The layer sits *outside* the schema's own auth resolution — it reads
//! the raw HTTP `Authorization` header directly, before `CratestackContext` auth
//! ever runs. So a schema whose `AuthProvider` accepts every request
//! regardless of headers (as below) is exactly the right fixture: it
//! isolates the layer's own identity derivation from the schema's,
//! confirming the refusal below is the layer's decision, not the schema's.

use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::extract::ConnectInfo;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{CratestackContext, CratestackError};
use cratestack_axum::idempotency::{IdempotencyLayer, IdempotencyStore, ReservationOutcome};
use cratestack_axum::ratelimit::{InMemoryRateLimitStore, RateLimitConfig, RateLimitLayer};
use cratestack_codec_json::JsonCodec;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tower::ServiceExt;

include_server_schema!("tests/fixtures/no_database_procedures.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CratestackError>,
    > + Send {
        async move {
            Ok(cratestack_schema::PingReply {
                echo: args.args.message,
            })
        }
    }
}

/// Accepts every request regardless of the raw HTTP `Authorization` header
/// — this schema's notion of "authenticated" has nothing to do with the
/// `RateLimitLayer`'s own header check, by design (see module doc).
#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

fn build_rate_limited_router(config: RateLimitConfig) -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    let store = Arc::new(InMemoryRateLimitStore::default());
    cratestack_schema::axum::router(
        db,
        Procedures,
        (),
        JsonCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(RateLimitLayer::new(store, config))
}

fn ping_request() -> Request<Body> {
    Request::post("/$procs/ping")
        .header("content-type", JsonCodec::CONTENT_TYPE)
        .header("accept", JsonCodec::CONTENT_TYPE)
        .body(Body::from(r#"{"args":{"message":"hi"}}"#))
        .expect("request")
}

async fn status_and_body(response: cratestack::axum::http::Response<Body>) -> (StatusCode, String) {
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 64 * 1024).await.expect("read body");
    (
        parts.status,
        std::str::from_utf8(&bytes).expect("utf8").to_owned(),
    )
}

/// **The decisive test.** Two distinct callers, neither carrying an
/// `Authorization` header, hitting a router served the way every shipped
/// example in this repository actually serves one (`into_make_service()`,
/// *not* `into_make_service_with_connect_info`) — i.e. the real,
/// undocumented, un-overridden default. Before cratestack#416's fix, both
/// requests would have silently landed in the same `"anonymous"` bucket:
/// caller B could exhaust caller A's rate-limit budget just by racing it,
/// with no way for either to know. The fix must not let that collision
/// happen — and it must not silently paper over the gap by picking a key
/// out of thin air either, so both are refused outright.
#[tokio::test]
async fn default_config_never_pools_two_unauthenticated_callers_into_one_bucket() {
    let router = build_rate_limited_router(RateLimitConfig::new(5, 1.0));

    let caller_a = router.clone().oneshot(ping_request()).await.expect("send");
    let caller_b = router.clone().oneshot(ping_request()).await.expect("send");

    let (status_a, body_a) = status_and_body(caller_a).await;
    let (status_b, body_b) = status_and_body(caller_b).await;

    assert_eq!(
        status_a,
        StatusCode::PRECONDITION_FAILED,
        "caller A: an unverifiable identity must be refused, not pooled into a shared bucket \
         (got body: {body_a})",
    );
    assert_eq!(
        status_b,
        StatusCode::PRECONDITION_FAILED,
        "caller B: an unverifiable identity must be refused, not pooled into a shared bucket \
         (got body: {body_b})",
    );
    // Neither response is a live `ping` reply nor a 429 — both distinct
    // requests were rejected identically and independently, proving there
    // is no shared "anonymous" bucket for one caller to exhaust on the
    // other's behalf.
    assert!(
        !body_a.contains("echo") && !body_b.contains("echo"),
        "neither refusal should carry a live procedure response",
    );
}

/// Positive control for the test above: once the operator actually wires
/// `ConnectInfo` (simulated here via the same request-extension mechanism
/// `into_make_service_with_connect_info` populates), two distinct peers
/// get independent buckets and are served normally — the fix doesn't
/// disable the layer, it only refuses the specific case where identity is
/// unverifiable.
#[tokio::test]
async fn distinct_peers_with_connect_info_get_independent_buckets() {
    let router = build_rate_limited_router(RateLimitConfig::new(1, 0.001));

    let peer_a: std::net::SocketAddr = "203.0.113.10:1".parse().unwrap();
    let peer_b: std::net::SocketAddr = "203.0.113.20:1".parse().unwrap();

    let mut req_a = ping_request();
    req_a.extensions_mut().insert(ConnectInfo(peer_a));
    let mut req_b = ping_request();
    req_b.extensions_mut().insert(ConnectInfo(peer_b));

    let resp_a = router.clone().oneshot(req_a).await.expect("send a");
    let resp_b = router.clone().oneshot(req_b).await.expect("send b");

    assert_eq!(
        resp_a.status(),
        StatusCode::OK,
        "peer A must be served independently of peer B's budget",
    );
    assert_eq!(
        resp_b.status(),
        StatusCode::OK,
        "peer B must be served independently of peer A's budget",
    );
}

// -----------------------------------------------------------------------------
// The `IdempotencyLayer` side of the same acceptance criteria: two distinct
// unauthenticated callers reusing the same `Idempotency-Key` must not have
// one replay the other's response. An in-memory `IdempotencyStore` double
// stands in for the sqlx/redis implementations this crate doesn't depend
// on (same pattern `cratestack-axum`'s own `tests_stream_bypass` uses).
// -----------------------------------------------------------------------------

#[derive(Default)]
struct InMemoryIdempotencyStore {
    entries: Mutex<HashMap<(String, String), Entry>>,
}

struct Entry {
    token: uuid::Uuid,
    hash: [u8; 32],
    record: Option<cratestack_axum::idempotency::IdempotencyRecord>,
}

#[async_trait::async_trait]
impl IdempotencyStore for InMemoryIdempotencyStore {
    async fn reserve_or_fetch(
        &self,
        principal: &str,
        key: &str,
        request_hash: [u8; 32],
        _expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CratestackError> {
        let mut entries = self.entries.lock().unwrap();
        let map_key = (principal.to_owned(), key.to_owned());
        match entries.get(&map_key) {
            None => {
                let token = uuid::Uuid::new_v4();
                entries.insert(
                    map_key,
                    Entry {
                        token,
                        hash: request_hash,
                        record: None,
                    },
                );
                Ok(ReservationOutcome::Reserved { token })
            }
            Some(entry) if entry.hash != request_hash => Ok(ReservationOutcome::Conflict),
            Some(entry) => Ok(match &entry.record {
                Some(record) => ReservationOutcome::Replay(record.clone()),
                None => ReservationOutcome::InFlight,
            }),
        }
    }

    async fn complete(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
        status: u16,
        headers: &[u8],
        body: &[u8],
    ) -> Result<(), CratestackError> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(&(principal.to_owned(), key.to_owned()))
            && entry.token == token
        {
            entry.record = Some(cratestack_axum::idempotency::IdempotencyRecord {
                key: key.to_owned(),
                principal_fingerprint: principal.to_owned(),
                request_hash: entry.hash,
                response_status: status,
                response_headers: headers.to_vec(),
                response_body: body.to_vec(),
                created_at: SystemTime::now(),
                expires_at: SystemTime::now(),
            });
        }
        Ok(())
    }

    async fn release(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
    ) -> Result<(), CratestackError> {
        let mut entries = self.entries.lock().unwrap();
        let map_key = (principal.to_owned(), key.to_owned());
        if entries
            .get(&map_key)
            .is_some_and(|entry| entry.token == token)
        {
            entries.remove(&map_key);
        }
        Ok(())
    }
}

fn build_idempotent_router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    let store = Arc::new(InMemoryIdempotencyStore::default());
    cratestack_schema::axum::router(
        db,
        Procedures,
        (),
        JsonCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(IdempotencyLayer::new(store, Duration::from_secs(60)))
}

fn ping_request_with_idempotency_key(key: &str) -> Request<Body> {
    Request::post("/$procs/ping")
        .header("content-type", JsonCodec::CONTENT_TYPE)
        .header("accept", JsonCodec::CONTENT_TYPE)
        .header("idempotency-key", key)
        .body(Body::from(r#"{"args":{"message":"hi"}}"#))
        .expect("request")
}

/// **The decisive test, idempotency side.** Two distinct callers, neither
/// carrying an `Authorization` header, reuse the same `Idempotency-Key`
/// under the real default (no `ConnectInfo` wired). Before cratestack#416's
/// fix, caller B's request would have replayed caller A's cached response
/// (or vice versa) — a genuine cross-caller data leak, not just a
/// throttling nuisance. The fix must not let that collision happen.
#[tokio::test]
async fn default_config_never_replays_one_unauthenticated_callers_response_for_another() {
    let router = build_idempotent_router();

    let caller_a = router
        .clone()
        .oneshot(ping_request_with_idempotency_key("shared-key"))
        .await
        .expect("send");
    let caller_b = router
        .clone()
        .oneshot(ping_request_with_idempotency_key("shared-key"))
        .await
        .expect("send");

    let (status_a, body_a) = status_and_body(caller_a).await;
    let (status_b, body_b) = status_and_body(caller_b).await;

    assert_eq!(
        status_a,
        StatusCode::PRECONDITION_FAILED,
        "caller A: an unverifiable identity must be refused, not given a shared idempotency \
         namespace (got body: {body_a})",
    );
    assert_eq!(
        status_b,
        StatusCode::PRECONDITION_FAILED,
        "caller B: an unverifiable identity must be refused, not given a shared idempotency \
         namespace (got body: {body_b})",
    );
    assert!(
        body_a.contains("no verifiable caller identity")
            && body_b.contains("no verifiable caller identity"),
        "both refusals must be the identity-verification error, not an \
         idempotency-key-conflict or replay artifact — got A: {body_a:?}, B: {body_b:?}",
    );
    // Neither is a replay of the other: this is checked implicitly above
    // (a replay would carry `idempotency-replayed: true` and a live `echo`
    // body, not the precondition-failed refusal), but assert it directly
    // too since that's the exact property the ticket is about.
    assert!(
        !body_a.contains("echo"),
        "caller A must not receive a live/replayed procedure response",
    );
    assert!(
        !body_b.contains("echo"),
        "caller B must not receive a live/replayed procedure response",
    );
}

/// Positive control for the test above: with `ConnectInfo` wired, two
/// distinct peers reusing the same `Idempotency-Key` are treated as
/// distinct principals — each runs the handler and gets its own record,
/// proving the fix doesn't disable idempotency, only the unverifiable case.
#[tokio::test]
async fn distinct_peers_with_connect_info_do_not_share_an_idempotency_namespace() {
    let router = build_idempotent_router();

    let peer_a: std::net::SocketAddr = "203.0.113.30:1".parse().unwrap();
    let peer_b: std::net::SocketAddr = "203.0.113.40:1".parse().unwrap();

    let mut req_a = ping_request_with_idempotency_key("shared-key-2");
    req_a.extensions_mut().insert(ConnectInfo(peer_a));
    let mut req_b = ping_request_with_idempotency_key("shared-key-2");
    req_b.extensions_mut().insert(ConnectInfo(peer_b));

    let resp_a = router.clone().oneshot(req_a).await.expect("send a");
    let resp_b = router.clone().oneshot(req_b).await.expect("send b");

    assert_eq!(resp_a.status(), StatusCode::OK);
    assert_eq!(resp_b.status(), StatusCode::OK);
    assert!(
        resp_a.headers().get("idempotency-replayed").is_none(),
        "peer A must get a live response, not a replay of peer B's",
    );
    assert!(
        resp_b.headers().get("idempotency-replayed").is_none(),
        "peer B must get a live response, not a replay of peer A's — distinct peers must not \
         share an idempotency namespace",
    );
}
