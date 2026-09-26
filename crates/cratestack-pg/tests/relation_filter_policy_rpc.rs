//! GHSA-p55v-6xv5-93p3, RPC transport counterpart of
//! `relation_filter_policy.rs`: `model.<M>.list` synthesizes query pairs
//! and re-enters the REST parser, so a relation filter/sort over a related
//! model the caller cannot read must behave as though the related row does
//! not exist on this transport too — while a readable one still works.

mod support;

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::Request;
use cratestack::include_server_schema;
use cratestack::rpc::{RpcListInput, RpcListPredicate};
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use support::pg;
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/relation_filter_policy_rpc.cstack",
    db = Postgres
);

#[derive(Clone)]
struct CallerOne;

impl AuthProvider for CallerOne {
    type Error = cratestack::CratestackError;
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

#[derive(Clone)]
struct NoProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

async fn list(router: &cratestack::axum::Router, input: RpcListInput) -> (u16, Vec<i64>) {
    let frame = CborCodec.encode(&input).expect("encode");
    let response = router
        .clone()
        .oneshot(
            Request::post("/rpc/model.RpcRfLink.list")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(frame))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let rows: Vec<cratestack::serde_json::Value> = CborCodec.decode(&bytes).unwrap_or_default();
    (
        status,
        rows.iter().filter_map(|row| row["id"].as_i64()).collect(),
    )
}

fn filter(key: &str, value: &str) -> RpcListInput {
    RpcListInput {
        filters: vec![RpcListPredicate {
            key: key.to_owned(),
            value: value.to_owned(),
        }],
        ..Default::default()
    }
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    for statement in [
        "DROP TABLE IF EXISTS rpc_rf_links, rpc_rf_hiddens, rpc_rf_publics",
        "CREATE TABLE rpc_rf_hiddens (id BIGINT PRIMARY KEY, code TEXT NOT NULL, score BIGINT NOT NULL)",
        "CREATE TABLE rpc_rf_publics (id BIGINT PRIMARY KEY, code TEXT NOT NULL)",
        "CREATE TABLE rpc_rf_links (id BIGINT PRIMARY KEY, hidden_id BIGINT NOT NULL, \
         public_id BIGINT NOT NULL)",
        "INSERT INTO rpc_rf_hiddens VALUES (40, 'CODE-A', 10), (41, 'CODE-B', 20)",
        "INSERT INTO rpc_rf_publics VALUES (97, 'P-A'), (98, 'P-B')",
        "INSERT INTO rpc_rf_links VALUES (50, 40, 97), (51, 41, 98)",
    ] {
        cratestack::sqlx::query(statement)
            .execute(pool)
            .await
            .expect(statement);
    }
}

fn sorted((status, mut ids): (u16, Vec<i64>)) -> (u16, Vec<i64>) {
    ids.sort_unstable();
    (status, ids)
}

#[tokio::test]
async fn rpc_relation_filter_and_sort_respect_the_related_read_policy() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    seed(&test_pg.pool).await;
    let router = cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build(),
        NoProcedures,
        (),
        CborCodec,
        CallerOne,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let where_of = |expr: &str| RpcListInput {
        where_expr: Some(expr.to_owned()),
        ..Default::default()
    };
    let or_of = |expr: &str| RpcListInput {
        or: Some(expr.to_owned()),
        ..Default::default()
    };
    let sort_of = |expr: &str| RpcListInput {
        sort: Some(expr.to_owned()),
        ..Default::default()
    };
    let none = (200, Vec::<i64>::new());

    // Hidden related rows never match, on every RPC list input slot.
    assert_eq!(
        sorted(list(&router, filter("hidden.code", "CODE-A")).await),
        none
    );
    assert_eq!(
        sorted(list(&router, filter("hidden.code__ne", "NOPE")).await),
        none
    );
    assert_eq!(
        sorted(list(&router, where_of("hidden.code=CODE-A")).await),
        none
    );
    assert_eq!(
        sorted(list(&router, or_of("hidden.code=CODE-A|id=0")).await),
        none
    );
    // A hidden sort key reads as NULL: both directions fall to the id tiebreak.
    assert_eq!(
        list(&router, sort_of("hidden.score,id")).await,
        (200, vec![50, 51])
    );
    assert_eq!(
        list(&router, sort_of("-hidden.score,id")).await,
        (200, vec![50, 51])
    );

    // Positive controls: a readable related row filters and sorts.
    assert_eq!(
        sorted(list(&router, filter("public.code", "P-A")).await),
        (200, vec![50])
    );
    assert_eq!(
        sorted(list(&router, where_of("public.code=P-B")).await),
        (200, vec![51])
    );
    assert_eq!(
        sorted(list(&router, or_of("public.code=P-A|id=0")).await),
        (200, vec![50])
    );
    assert_eq!(
        list(&router, sort_of("-public.code,id")).await,
        (200, vec![51, 50])
    );
}
