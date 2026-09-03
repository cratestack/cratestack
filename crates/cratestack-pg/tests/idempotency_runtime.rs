//! Runtime test for `@no_idempotency` (#876, ADR 0015 slice 1): a
//! procedure carrying the attribute makes **no reservation**, while an
//! un-annotated mutation still does — on both transports.
//!
//! Modelled on `rate_limit_runtime.rs`, which does the same job for
//! `@no_rate_limit`. Like that test, it drives real requests and asserts on
//! observable behaviour rather than inspecting generated tokens: the
//! codegen assertions already live in
//! `cratestack-macros/src/transport/{op_descriptors,rest}/tests_idempotency.rs`,
//! and a snapshot of emitted tokens cannot tell you the runtime reads them.
//! The decisive property is that deleting the
//! `procedure_idempotent_by_default` call in codegen makes this file fail.
//!
//! No live Postgres is required, for the same two reasons
//! `rate_limit_runtime.rs` gives: neither procedure touches `db`, and
//! `PgPoolOptions::connect_lazy` never opens a connection. The idempotency
//! store here is deliberately in-memory rather than
//! `SqlxIdempotencyStore` — this test is about *admission* (does a
//! reservation happen at all), and the PG-backed storage semantics it
//! would otherwise duplicate are already covered, unedited, by
//! `banking_idempotency.rs`.
//!
//! Each transport gets its own `include_server_schema!` inside its own
//! module, because a schema is REST or RPC and never both.

#![cfg(feature = "codec-json")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::extract::ConnectInfo;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_axum::idempotency::IdempotencyLayer;
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

mod support;

use support::idempotency_store::InMemoryIdempotencyStore;

/// cratestack#416: the default principal fingerprint refuses a request
/// with neither an `Authorization` header nor a `ConnectInfo<SocketAddr>`
/// peer, and `oneshot` never populates `ConnectInfo` on its own.
fn with_peer(mut req: Request<Body>) -> Request<Body> {
    let peer: std::net::SocketAddr = "192.0.2.90:1".parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    req
}

/// `{"args": {"nonce": ...}}` — the generated `Args { pub args: PingArgs }`
/// shape, since the declared argument is literally named `args`.
fn body(nonce: &str) -> String {
    format!(r#"{{"args":{{"nonce":"{nonce}"}}}}"#)
}

/// Counts handler invocations so "no reservation was taken" can be
/// asserted positively (the annotated procedure runs *twice*) rather than
/// only by the absence of a 422.
///
/// Per-registry-instance rather than a pair of `static`s: `cargo test`
/// runs these cases on separate threads by default, and file-global
/// counters made every assertion depend on which other tests happened to
/// be in flight. (Measured: `left: 5, right: 2`.)
#[derive(Clone, Default)]
pub struct Calls {
    transfer: Arc<AtomicUsize>,
    notify: Arc<AtomicUsize>,
}

impl Calls {
    pub fn transfer(&self) -> usize {
        self.transfer.load(Ordering::SeqCst)
    }

    pub fn notify(&self) -> usize {
        self.notify.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
struct AlwaysAuth;

impl AuthProvider for AlwaysAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

/// The two handler impls are identical for both transports — the
/// generated `ProcedureRegistry` trait has the same shape either way — so
/// they are written once and expanded into each schema module. A
/// `$schema:path` parameter is not usable here (a `path` fragment cannot
/// be followed by `::`), and it is not needed: `macro_rules!` resolves
/// module paths at the expansion site, so the bare `cratestack_schema`
/// below binds to whichever module the macro is invoked in.
macro_rules! procedures_impl {
    () => {
        #[derive(Clone)]
        pub struct Procedures(pub super::Calls);

        impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
            fn transfer(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: cratestack_schema::procedures::transfer::Args,
                _authorized: cratestack_schema::procedures::transfer::Authorized,
            ) -> impl core::future::Future<
                Output = Result<cratestack_schema::procedures::transfer::Output, CratestackError>,
            > + Send {
                self.0.transfer.fetch_add(1, Ordering::SeqCst);
                core::future::ready(Ok(args.args))
            }

            fn notify(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: cratestack_schema::procedures::notify::Args,
                _authorized: cratestack_schema::procedures::notify::Authorized,
            ) -> impl core::future::Future<
                Output = Result<cratestack_schema::procedures::notify::Output, CratestackError>,
            > + Send {
                self.0.notify.fetch_add(1, Ordering::SeqCst);
                core::future::ready(Ok(args.args))
            }
        }
    };
}

pub mod rpc {
    use super::*;
    include_server_schema!(
        "tests/fixtures/idempotency_runtime_rpc.cstack",
        db = Postgres
    );
    procedures_impl!();
}

pub mod rest {
    use super::*;
    include_server_schema!(
        "tests/fixtures/idempotency_runtime_rest.cstack",
        db = Postgres
    );
    procedures_impl!();
}

fn lazy_pool() -> cratestack::sqlx::PgPool {
    PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse")
}

/// The two requests every case sends: same `Idempotency-Key`, **different
/// bodies**. Under a reservation the second is a conflict; without one it
/// simply runs.
async fn two_requests_one_key(
    router: cratestack::axum::Router,
    uri: &str,
    key: &str,
) -> (StatusCode, StatusCode, String) {
    let send = |body_text: String| {
        let router = router.clone();
        let uri = uri.to_owned();
        let key = key.to_owned();
        async move {
            router
                .oneshot(with_peer(
                    Request::post(&uri)
                        .header("content-type", "application/json")
                        .header("accept", "application/json")
                        .header("idempotency-key", &key)
                        .body(Body::from(body_text))
                        .expect("request should build"),
                ))
                .await
                .expect("router is infallible")
        }
    };

    let first = send(body("one")).await;
    let first_status = first.status();
    let second = send(body("two")).await;
    let second_status = second.status();
    let second_body = to_bytes(second.into_body(), 64 * 1024)
        .await
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();

    (first_status, second_status, second_body)
}

/// Build an RPC router with the layer + resolver installed.
fn rpc_app() -> (
    cratestack::axum::Router,
    Arc<InMemoryIdempotencyStore>,
    Calls,
) {
    use cratestack_axum::idempotency::build_rpc_op_resolver;

    let db = rpc::cratestack_schema::Cratestack::builder(lazy_pool()).build();
    let store = Arc::new(InMemoryIdempotencyStore::default());
    let calls = Calls::default();
    let router = rpc::cratestack_schema::axum::rpc_router(
        db,
        rpc::Procedures(calls.clone()),
        (),
        JsonCodec,
        AlwaysAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(
        IdempotencyLayer::new(store.clone(), std::time::Duration::from_secs(60))
            .with_op_resolver(build_rpc_op_resolver(rpc::cratestack_schema::axum::OPS)),
    );
    (router, store, calls)
}

/// Build a REST router with the layer + resolver installed.
///
/// `route_layer`, not `layer`: `MatchedPath` is populated by both, but
/// `route_layer` leaves 404s out of the middleware entirely, which is the
/// mount the README recommends for the REST resolver.
fn rest_app() -> (
    cratestack::axum::Router,
    Arc<InMemoryIdempotencyStore>,
    Calls,
) {
    use cratestack_axum::idempotency::build_rest_op_resolver;

    let db = rest::cratestack_schema::Cratestack::builder(lazy_pool()).build();
    let store = Arc::new(InMemoryIdempotencyStore::default());
    let calls = Calls::default();
    let router = rest::cratestack_schema::axum::router(
        db,
        rest::Procedures(calls.clone()),
        (),
        JsonCodec,
        AlwaysAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .route_layer(
        IdempotencyLayer::new(store.clone(), std::time::Duration::from_secs(60)).with_op_resolver(
            build_rest_op_resolver(rest::cratestack_schema::axum::ROUTE_TRANSPORTS),
        ),
    );
    (router, store, calls)
}

// ------------------------------------------------------------------ RPC

/// AC: a `@no_idempotency` procedure takes no reservation. Two requests
/// with the same key and *different* bodies both execute — which is only
/// possible if nothing reserved the key on the first one.
#[tokio::test]
async fn rpc_no_idempotency_procedure_makes_no_reservation() {
    let (router, store, calls) = rpc_app();

    let (first, second, body) =
        two_requests_one_key(router, "/rpc/procedure.notify", "rpc-notify-key").await;

    assert_eq!(first, StatusCode::OK, "first call should run");
    assert_eq!(
        second,
        StatusCode::OK,
        "@no_idempotency: a second call under the same key with a DIFFERENT body \
         must run too, not conflict. Got body: {body}"
    );
    assert_eq!(calls.notify(), 2, "both calls must reach the handler");
    assert_eq!(
        store.reserve_calls(),
        0,
        "the store must never be asked to reserve for a @no_idempotency op"
    );
}

/// The negative control that makes the test above mean something: the
/// un-annotated twin, same schema, same key, same two bodies, still
/// conflicts. Without this, a blanket "idempotency is off" regression
/// would pass.
#[tokio::test]
async fn rpc_ordinary_mutation_still_conflicts_on_a_reused_key() {
    let (router, store, calls) = rpc_app();

    let (first, second, body) =
        two_requests_one_key(router, "/rpc/procedure.transfer", "rpc-transfer-key").await;

    assert_eq!(first, StatusCode::OK, "first call should run");
    assert_eq!(
        second,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an un-annotated mutation must still reserve, so a reused key with a \
         different body is a conflict. Got body: {body}"
    );
    assert!(
        body.contains("idempotency_key_conflict"),
        "the 422 must carry the IETF draft's code: {body}"
    );
    assert_eq!(
        calls.transfer(),
        1,
        "the second call must NOT reach the handler"
    );
    assert!(
        store.reserve_calls() >= 1,
        "an un-annotated mutation must reach the store"
    );
}

// ----------------------------------------------------------------- REST

/// Transport parity: the identical pair of assertions over REST. #474's
/// lesson is that a fix landing on one transport and no-oping on the other
/// looks exactly like a passing test suite.
#[tokio::test]
async fn rest_no_idempotency_procedure_makes_no_reservation() {
    let (router, store, calls) = rest_app();

    let (first, second, body) =
        two_requests_one_key(router, "/$procs/notify", "rest-notify-key").await;

    assert_eq!(first, StatusCode::OK, "first call should run");
    assert_eq!(
        second,
        StatusCode::OK,
        "@no_idempotency over REST: a second call under the same key with a \
         DIFFERENT body must run too, not conflict. Got body: {body}"
    );
    assert_eq!(calls.notify(), 2, "both calls must reach the handler");
    assert_eq!(
        store.reserve_calls(),
        0,
        "the store must never be asked to reserve for a @no_idempotency route"
    );
}

#[tokio::test]
async fn rest_ordinary_mutation_still_conflicts_on_a_reused_key() {
    let (router, store, calls) = rest_app();

    let (first, second, body) =
        two_requests_one_key(router, "/$procs/transfer", "rest-transfer-key").await;

    assert_eq!(first, StatusCode::OK, "first call should run");
    assert_eq!(
        second,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an un-annotated mutation must still reserve over REST too. Got body: {body}"
    );
    assert!(
        body.contains("idempotency_key_conflict"),
        "the 422 must carry the IETF draft's code: {body}"
    );
    assert_eq!(
        calls.transfer(),
        1,
        "the second call must NOT reach the handler"
    );
    assert!(
        store.reserve_calls() >= 1,
        "an un-annotated mutation must reach the store"
    );
}

/// Installing no resolver is the byte-identity configuration, and it must
/// keep reserving for *everything* — including the `@no_idempotency`
/// procedure. This is what lets an existing consumer upgrade without any
/// behaviour change at all.
#[tokio::test]
async fn without_a_resolver_even_the_annotated_procedure_still_reserves() {
    let db = rpc::cratestack_schema::Cratestack::builder(lazy_pool()).build();
    let store = Arc::new(InMemoryIdempotencyStore::default());
    let router = rpc::cratestack_schema::axum::rpc_router(
        db,
        rpc::Procedures(Calls::default()),
        (),
        JsonCodec,
        AlwaysAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(IdempotencyLayer::new(
        store.clone(),
        std::time::Duration::from_secs(60),
    ));

    let (first, second, body) =
        two_requests_one_key(router, "/rpc/procedure.notify", "no-resolver-key").await;

    assert_eq!(first, StatusCode::OK);
    assert_eq!(
        second,
        StatusCode::UNPROCESSABLE_ENTITY,
        "with no resolver installed every op is unresolved, and unresolved \
         reserves — this is the configuration every pre-existing consumer is \
         in, and its behaviour must be unchanged. Got body: {body}"
    );
    assert!(
        store.reserve_calls() >= 1,
        "the default resolver must still reach the store"
    );
}

// ------------------------------------------- request-body cap symmetry

/// A body comfortably over the idempotency middleware's 2 MiB
/// `MAX_BODY_BYTES` buffer.
///
/// The router's own `DefaultBodyLimit` is raised to 8 MiB in the helper
/// below so this test isolates the *idempotency* cap. Both constants are
/// 2 MiB by default and `cratestack-core/src/limits.rs` asserts
/// `DEFAULT_BODY_LIMIT_BYTES <= MAX_BODY_BYTES` at compile time, so
/// without that the router would reject first and this test would prove
/// nothing about the middleware.
fn oversized_body() -> String {
    let filler = "x".repeat(3 * 1024 * 1024);
    format!(r#"{{"args":{{"nonce":"{filler}"}}}}"#)
}

const ROOMY_LIMIT: usize = 8 * 1024 * 1024;

/// One oversized POST carrying an `Idempotency-Key`, against an RPC
/// router whose own body limit is out of the way.
async fn oversized_post(uri: &str, with_resolver: bool) -> (StatusCode, String) {
    use cratestack_axum::idempotency::build_rpc_op_resolver;

    let db = rpc::cratestack_schema::Cratestack::builder(lazy_pool()).build();
    let store = Arc::new(InMemoryIdempotencyStore::default());
    let base = rpc::cratestack_schema::axum::rpc_router(
        db,
        rpc::Procedures(Calls::default()),
        (),
        JsonCodec,
        AlwaysAuth,
        ROOMY_LIMIT,
    );
    let layer = IdempotencyLayer::new(store, std::time::Duration::from_secs(60));
    let router = if with_resolver {
        base.layer(layer.with_op_resolver(build_rpc_op_resolver(rpc::cratestack_schema::axum::OPS)))
    } else {
        base.layer(layer)
    };

    let response = router
        .oneshot(with_peer(
            Request::post(uri)
                .header("content-type", "application/json")
                .header("accept", "application/json")
                .header("idempotency-key", "oversized-key")
                .body(Body::from(oversized_body()))
                .expect("request should build"),
        ))
        .await
        .expect("router is infallible");
    let status = response.status();
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    (status, body)
}

/// A `@no_idempotency` POST larger than the idempotency buffer must
/// succeed, exactly as it would with no `Idempotency-Key` header at all.
///
/// Before the short-circuit moved above the body read, this request paid
/// the 2 MiB cap to compute a fingerprint that admission was going to
/// throw away — i.e. the header, which the schema says does nothing for
/// this op, was the only reason the request failed.
#[tokio::test]
async fn oversized_body_succeeds_for_a_no_idempotency_procedure() {
    let (status, body) = oversized_post("/rpc/procedure.notify", true).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a @no_idempotency op must not pay the idempotency buffer cap. Got: {body}"
    );
}

/// Negative control: the un-annotated twin still pays the cap, so the
/// test above is measuring the bypass and not a raised limit.
#[tokio::test]
async fn oversized_body_still_rejected_for_a_participating_procedure() {
    let (status, body) = oversized_post("/rpc/procedure.transfer", true).await;
    assert_ne!(
        status,
        StatusCode::OK,
        "a participating mutation must still be capped by the idempotency buffer"
    );
    assert!(
        body.contains("idempotency buffer limit"),
        "the rejection must be the idempotency buffer's, not the router's: {body}"
    );
}

/// Byte-identity control: with NO resolver installed, even the annotated
/// procedure is unresolved, never short-circuits, and still pays the cap —
/// which is what every pre-existing consumer sees.
#[tokio::test]
async fn oversized_body_still_rejected_when_no_resolver_is_installed() {
    let (status, body) = oversized_post("/rpc/procedure.notify", false).await;
    assert_ne!(
        status,
        StatusCode::OK,
        "with no resolver the op is unresolved, so the short-circuit must not fire"
    );
    assert!(body.contains("idempotency buffer limit"), "got: {body}");
}
