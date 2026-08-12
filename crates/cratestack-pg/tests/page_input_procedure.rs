//! Regression test for the built-in `PageInput` procedure-argument type:
//! `{ limit: Int?, offset: Int? }`, decoded off the wire in
//! `#[serde(rename_all = "camelCase")]` lockstep with `PageInfo`'s own
//! fields, with `PageInput::resolve` applying the same clamp rule
//! generated `list` routes already use. `datasource { provider = "none" }`
//! keeps this Postgres-free, following `no_database_procedures.rs`'s
//! precedent.

use cratestack::CoolCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{CoolContext, CoolError, PageInput};
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/page_input_procedure.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn list_feed(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::list_feed::Args,
        _authorized: cratestack_schema::procedures::list_feed::Authorized,
    ) -> Result<cratestack_schema::procedures::list_feed::Output, CoolError> {
        let (limit, offset) = args.page.resolve(50);
        Ok(cratestack_schema::FeedReply { limit, offset })
    }
}

#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = CoolError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        core::future::ready(Ok(CoolContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

fn build_router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::router(
        db,
        Procedures,
        JsonCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

#[test]
fn page_input_arg_decodes_from_camel_case_json() {
    let args: cratestack_schema::procedures::list_feed::Args =
        serde_json::from_value(serde_json::json!({ "page": { "limit": 10, "offset": 20 } }))
            .expect("PageInput arg should decode from camelCase JSON");

    assert_eq!(args.page.limit, Some(10));
    assert_eq!(args.page.offset, Some(20));
}

#[test]
fn page_input_resolve_clamps_and_defaults() {
    let unset = PageInput::default();
    assert_eq!(unset.resolve(50), (50, 0));

    let out_of_range = PageInput {
        limit: Some(9_999),
        offset: Some(-5),
    };
    assert_eq!(out_of_range.resolve(50), (50, 0));

    let in_range = PageInput {
        limit: Some(5),
        offset: Some(15),
    };
    assert_eq!(in_range.resolve(50), (5, 15));
}

/// The story's headline evidence: a `PageInput`-typed procedure argument
/// round-trips a real HTTP call end to end, decoded and resolved exactly
/// like a generated `list` route's own `limit`/`offset` handling.
#[tokio::test]
async fn page_input_router_round_trips_over_http() {
    let app = build_router();

    let body = serde_json::json!({ "page": { "limit": 5, "offset": 15 } });
    let response = app
        .oneshot(
            Request::post("/$procs/listFeed")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply["limit"], 5);
    assert_eq!(reply["offset"], 15);
}
