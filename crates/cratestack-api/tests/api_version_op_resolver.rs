//! `@api_version` and the REST op resolver: `@no_idempotency` must take
//! effect on a versioned procedure.
//!
//! `build_rest_op_resolver` matches the request's `MatchedPath` against
//! `ROUTE_TRANSPORTS` by exact string. Before the fix the descriptor named
//! `/$procs/ping` while the router mounted `/v2/$procs/ping`, so the lookup
//! missed and the op resolved as `OpAdmission::unresolved()`: reserve,
//! whatever the schema said. The rate-limit filter (`build_rest_ops_filter`)
//! is a projection of this same resolver, so `@no_rate_limit` had the same
//! miss; it needs the `rate_limit` feature this facade does not forward. This drives the real generated
//! router, so `MatchedPath` is whatever axum actually matched.

use std::sync::{Arc, Mutex};

use cratestack::axum::body::Body;
use cratestack::axum::extract::Request;
use cratestack::axum::http::Request as HttpRequest;
use cratestack::axum::middleware::{Next, from_fn};
use cratestack::idempotency::{OpAdmission, build_rest_op_resolver};
use cratestack::{CratestackContext, CratestackError};
use tower::ServiceExt;

cratestack::include_server_schema!("tests/fixtures/api_version_opt_outs.cstack", db = None);

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

/// The resolver runs before dispatch, so the caller's identity is
/// irrelevant here; any `AuthProvider` will do.
fn anonymous(
    _headers: &cratestack::axum::http::HeaderMap,
) -> Result<CratestackContext, CratestackError> {
    Ok(CratestackContext::anonymous())
}

#[tokio::test]
async fn versioned_procedure_opt_outs_resolve_through_the_mounted_path() {
    let resolved: Arc<Mutex<Option<OpAdmission>>> = Arc::default();
    let sink = resolved.clone();
    let resolver = Arc::new(build_rest_op_resolver(
        cratestack_schema::axum::ROUTE_TRANSPORTS,
    ));
    let router = cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        cratestack_codec_json::JsonCodec,
        anonymous,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    // `route_layer` runs after routing, so `MatchedPath` is populated: the
    // same position an `IdempotencyLayer`/`RateLimitLayer` reads it from.
    .route_layer(from_fn(move |request: Request, next: Next| {
        let sink = sink.clone();
        let resolver = resolver.clone();
        async move {
            *sink.lock().unwrap() = Some(resolver(&request));
            next.run(request).await
        }
    }));

    let _ = router
        .oneshot(
            HttpRequest::post("/v2/$procs/ping")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"args":{"message":"hi"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    let admission = resolved
        .lock()
        .unwrap()
        .expect("the request must reach the mounted versioned route");
    assert_ne!(
        admission,
        OpAdmission::unresolved(),
        "the resolver must find ping's descriptor at /v2/$procs/ping"
    );
    assert!(admission.idempotent_by_default, "@no_idempotency must apply");
}
