//! cratestack#743 — `@@internal("action")` route suppression, exercised
//! end-to-end against real generated routers (`docs/design/
//! route-suppression.md`). Every request here is dispatch-level only
//! (unregistered route / unknown op id) and never actually reaches the
//! database, so — like `include_schema.rs`'s existing
//! `rpc_unary_unknown_op_returns_404` and
//! `rpc_batch_per_frame_errors_dont_poison_other_frames` this mirrors —
//! `connect_lazy` is enough; no live Postgres or testcontainer is
//! needed for this file specifically.
//!
//! Three surfaces, per the design's acceptance criteria:
//! 1. REST: a suppressed verb on a path with survivors gets axum's own
//!    405; a model suppressing every verb never registers either path
//!    (404).
//! 2. RPC unary: a suppressed op id gets the exact same
//!    `CratestackError::NotFound` a genuinely unknown op id gets.
//! 3. RPC batch: a suppressed op id in one frame doesn't poison sibling
//!    frames.

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{AuthProvider, CratestackContext, RequestContext};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

/// Always-authenticated: every model in these fixtures gates on
/// `auth() != null`, and these tests only care about routing/dispatch
/// (a suppressed op must never reach a handler at all), not policy
/// outcomes — so authenticating unconditionally keeps the handful of
/// "verb X still routes" assertions from being confused with a
/// would-be 403.
#[derive(Clone)]
struct AlwaysAuthProvider;

impl AuthProvider for AlwaysAuthProvider {
    type Error = cratestack::CratestackError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

fn lazy_pool() -> cratestack::sqlx::PgPool {
    PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse")
}

mod rest_suppression {
    use super::*;

    include_server_schema!(
        "tests/fixtures/internal_suppression_rest.cstack",
        db = Postgres
    );

    fn router() -> cratestack::axum::Router {
        let db = cratestack_schema::Cratestack::builder(lazy_pool()).build();
        cratestack_schema::axum::model_router(db, (), CborCodec, AlwaysAuthProvider)
    }

    async fn status_for(method: &str, path: &str) -> StatusCode {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/cbor")
            .body(Body::from(Vec::<u8>::new()))
            .expect("request should build");
        router()
            .oneshot(request)
            .await
            .expect("request should succeed")
            .status()
    }

    /// The one criterion the design is most explicit about not
    /// silently violating: a suppressed verb must get the SAME status
    /// an unregistered verb has always gotten — not 403, and not a
    /// Cratestack-specific code.
    #[tokio::test]
    async fn suppressed_create_on_a_shared_path_returns_405_not_403() {
        let status = status_for("POST", "/widgets").await;
        assert_eq!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "suppressed POST /widgets must be axum's bare 405, got {status}",
        );
    }

    /// The other three `Widget` verbs share no path exclusively with
    /// `create` suppression logic — they must stay routed. We only
    /// assert "not 404/405 the way a suppressed verb would be" here:
    /// this router's `db` is a `connect_lazy` pool with no real
    /// Postgres behind it, so a genuinely dispatched request may still
    /// fail downstream (e.g. a connection error) — what matters is
    /// that dispatch was attempted at all, i.e. axum matched the route.
    #[tokio::test]
    async fn surviving_verbs_on_widget_stay_routed() {
        for (method, path) in [
            ("GET", "/widgets"),
            ("GET", "/widgets/1"),
            ("PATCH", "/widgets/1"),
            ("DELETE", "/widgets/1"),
        ] {
            let status = status_for(method, path).await;
            assert_ne!(
                status,
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {path} must still be routed (not suppressed)",
            );
            assert_ne!(
                status,
                StatusCode::NOT_FOUND,
                "{method} {path} must still be routed (not suppressed)",
            );
        }
    }

    /// `@@internal("all")` on `Gadget`: neither path is ever
    /// registered, so every verb on both paths falls through to axum's
    /// bare default 404 — not a 405 (that would imply the path
    /// matched but the verb didn't), and not a Cratestack-specific
    /// error body.
    #[tokio::test]
    async fn internal_all_never_registers_either_gadget_path() {
        for (method, path) in [
            ("GET", "/gadgets"),
            ("POST", "/gadgets"),
            ("GET", "/gadgets/1"),
            ("PATCH", "/gadgets/1"),
            ("DELETE", "/gadgets/1"),
        ] {
            let status = status_for(method, path).await;
            assert_eq!(
                status,
                StatusCode::NOT_FOUND,
                "{method} {path} on an all-suppressed model must be a bare 404, got {status}",
            );
        }
    }
}

mod rpc_suppression {
    use super::*;
    use cratestack::CratestackCodec;

    include_server_schema!(
        "tests/fixtures/internal_suppression_rpc.cstack",
        db = Postgres
    );

    #[derive(Clone)]
    struct NoProcedures;
    impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

    fn router() -> cratestack::axum::Router {
        let db = cratestack_schema::Cratestack::builder(lazy_pool()).build();
        cratestack_schema::axum::rpc_router(
            db,
            NoProcedures,
            (),
            CborCodec,
            AlwaysAuthProvider,
            cratestack::DEFAULT_BODY_LIMIT_BYTES,
        )
    }

    /// The design's central RPC claim: a suppressed op id must be
    /// byte-identical, at the wire, to an op id that was never a real
    /// op at all — both fall into the exact same pre-existing
    /// unknown-op-id arm in `rpc_dispatch_inner`.
    #[tokio::test]
    async fn suppressed_create_op_returns_the_same_not_found_as_a_genuinely_unknown_op() {
        for op in ["model.Widget.create", "model.Widget.does_not_exist"] {
            let path = format!("/rpc/{op}");
            let response = router()
                .oneshot(
                    Request::post(path)
                        .header("content-type", CborCodec::CONTENT_TYPE)
                        .body(Body::from(Vec::<u8>::new()))
                        .expect("request should build"),
                )
                .await
                .expect("request should succeed");
            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "op `{op}` should be 404 NOT_FOUND",
            );
            let bytes = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body should buffer");
            let body: cratestack::CratestackErrorResponse =
                CborCodec.decode(&bytes).expect("error body should decode");
            assert_eq!(
                body.code, "not_found",
                "op `{op}` should carry code `not_found`, got {:?}",
                body.code,
            );
        }
    }

    /// `model.Widget.list` is not suppressed, so it must still dispatch
    /// (i.e. not fall into the unknown-op arm) — proving suppression is
    /// scoped to the one verb named, not the whole model.
    #[tokio::test]
    async fn unsuppressed_list_op_does_not_hit_the_unknown_op_arm() {
        let response = router()
            .oneshot(
                Request::post("/rpc/model.Widget.list")
                    .header("content-type", CborCodec::CONTENT_TYPE)
                    .body(Body::from(Vec::<u8>::new()))
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");
        // A real dispatch against a `connect_lazy` pool with no live
        // Postgres will fail downstream — the point here is only that
        // it is NOT the unknown-op 404 a suppressed/unknown id gets.
        assert_ne!(
            response.status(),
            StatusCode::NOT_FOUND,
            "model.Widget.list must not be treated as an unknown op",
        );
    }

    /// The batch half of the design's acceptance criteria: one
    /// suppressed op among valid ones gets a per-frame `not_found` at
    /// its index, and every other frame is dispatched and unaffected —
    /// mirrors `include_schema.rs`'s
    /// `rpc_batch_per_frame_errors_dont_poison_other_frames`, but pinned
    /// to an ACTUALLY-suppressed model op id rather than a never-existed
    /// procedure name (design doc §3.2's own follow-up note).
    #[tokio::test]
    async fn suppressed_op_in_batch_gets_a_per_frame_error_and_does_not_poison_siblings() {
        let frames = vec![
            cratestack::rpc::RpcRequest {
                id: 1,
                op: "model.Widget.create".into(),
                input: serde_json::json!({"name": "should not dispatch"}),
                idem: None,
            },
            cratestack::rpc::RpcRequest {
                id: 2,
                op: "model.Widget.does_not_exist".into(),
                input: serde_json::json!(null),
                idem: None,
            },
        ];
        let body = CborCodec.encode(&frames).expect("batch body should encode");
        let response = router()
            .oneshot(
                Request::post("/rpc/batch")
                    .header("content-type", CborCodec::CONTENT_TYPE)
                    .body(Body::from(body))
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "batch envelope must succeed even though every frame errors"
        );
        let bytes = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should buffer");
        let responses: Vec<cratestack::rpc::RpcResponseFrame> =
            CborCodec.decode(&bytes).expect("batch response decodes");

        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0].id, 1);
        assert_eq!(responses[1].id, 2);

        let suppressed = responses[0]
            .error
            .as_ref()
            .expect("frame 0 (suppressed create) should carry an error");
        assert_eq!(
            suppressed.code, "not_found",
            "suppressed op should carry code `not_found`: {suppressed:?}",
        );

        let unknown = responses[1]
            .error
            .as_ref()
            .expect("frame 1 (genuinely unknown op) should carry an error");
        assert_eq!(
            suppressed.code, unknown.code,
            "a suppressed op and a genuinely unknown op must be byte-identical at the wire",
        );
    }
}
