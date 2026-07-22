//! End-to-end proof that `@@paged` behaves identically on `transport
//! rpc` and REST — they share one generated dispatch function
//! (`handle_list_<plural>_dispatch`; RPC synthesizes a query string
//! from `RpcListInput` and calls straight into it), but until this
//! test nothing exercised that claim against the real generated
//! `rpc_router`. Also proves the #123 `MAX_LIST_LIMIT` cap applies to
//! RPC too, for the same reason.

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{AuthProvider, CoolCodec, CoolContext, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/transport_rpc.cstack", db = Postgres);

mod support;

use support::pg;

#[derive(Clone)]
struct AlwaysAuthProvider;

impl AuthProvider for AlwaysAuthProvider {
    type Error = cratestack::CoolError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        core::future::ready(Ok(CoolContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

#[derive(Clone)]
struct NoProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::ping::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, cratestack::CoolError>,
    > + Send {
        core::future::ready(Ok(args.args))
    }

    fn bump(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::bump::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::bump::Output, cratestack::CoolError>,
    > + Send {
        core::future::ready(Ok(args.args))
    }

    fn many_pings(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::many_pings::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::many_pings::Output, cratestack::CoolError>,
    > + Send {
        core::future::ready(Ok(vec![args.args]))
    }
}

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    cratestack::sqlx::query("DROP TABLE IF EXISTS paged_items")
        .execute(pool)
        .await
        .expect("drop paged_items");
    cratestack::sqlx::query(
        "CREATE TABLE paged_items (id BIGINT PRIMARY KEY, label TEXT NOT NULL)",
    )
    .execute(pool)
    .await
    .expect("create paged_items");
    for id in 1..=5 {
        cratestack::sqlx::query("INSERT INTO paged_items (id, label) VALUES ($1, $2)")
            .bind(id as i64)
            .bind(format!("item-{id}"))
            .execute(pool)
            .await
            .expect("seed paged_items");
    }
}

fn router(pool: cratestack::sqlx::PgPool) -> cratestack::axum::Router {
    cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder(pool).build(),
        NoProcedures,
        CborCodec,
        AlwaysAuthProvider,
    )
}

#[tokio::test]
async fn rpc_list_on_a_paged_model_returns_the_same_page_envelope_as_rest() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let router = router(pool.clone());
    let codec = CborCodec;
    let frame = codec
        .encode(&cratestack::rpc::RpcListInput {
            limit: Some(2),
            offset: Some(1),
            ..Default::default()
        })
        .expect("encode list input");

    let response = router
        .oneshot(
            Request::post("/rpc/model.PagedItem.list")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(frame))
                .expect("request should build"),
        )
        .await
        .expect("rpc dispatch should complete");
    assert_eq!(response.status(), StatusCode::OK);

    let body = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let page: cratestack::Page<cratestack_schema::PagedItem> =
        codec.decode(&body).expect("page should decode");

    assert_eq!(page.items.len(), 2, "limit=2 should return exactly 2 rows");
    assert_eq!(page.items[0].id, 2, "offset=1 should skip the first row");
    assert_eq!(page.items[1].id, 3);
    assert_eq!(page.total_count, Some(5), "totalCount ignores limit/offset");
    assert_eq!(page.page_info.limit, Some(2));
    assert_eq!(page.page_info.offset, Some(1));
    assert!(
        page.page_info.has_next_page,
        "2 more rows remain after offset=1, limit=2 (ids 4, 5)",
    );
    assert!(page.page_info.has_previous_page, "offset=1 > 0",);
}

#[tokio::test]
async fn rpc_list_rejects_limit_above_max_list_limit_same_as_rest() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let router = router(pool.clone());
    let codec = CborCodec;
    let frame = codec
        .encode(&cratestack::rpc::RpcListInput {
            limit: Some(cratestack::MAX_LIST_LIMIT + 1),
            ..Default::default()
        })
        .expect("encode list input");

    let response = router
        .oneshot(
            Request::post("/rpc/model.PagedItem.list")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(frame))
                .expect("request should build"),
        )
        .await
        .expect("rpc dispatch should complete");

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "MAX_LIST_LIMIT is enforced in the dispatch function RPC and REST share, \
         so an over-limit RPC list call must be rejected exactly like REST is",
    );
}
