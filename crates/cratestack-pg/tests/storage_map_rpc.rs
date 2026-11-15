//! RPC parity for `@@map` / `@map` (cratestack#1375): the op ids stay
//! `model.MapRpcBank.*` and the frames carry field names, while the statements
//! behind them run against `map_rpc_bank` and its mapped columns.

use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_cbor::CborCodec;
use serde_json::{Value as Json, json};
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/storage_map_rpc.cstack", db = Postgres);

mod support;

use support::pg;

#[derive(Clone)]
struct AlwaysAuthProvider;

impl AuthProvider for AlwaysAuthProvider {
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

#[derive(Clone)]
struct NoProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

async fn post(router: &Router, op: &str, body: Json) -> (StatusCode, Vec<u8>) {
    let response = router
        .clone()
        .oneshot(
            Request::post(format!("/rpc/{op}"))
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(CborCodec.encode(&body).expect("encode")))
                .expect("request"),
        )
        .await
        .expect("dispatch");
    let status = response.status();
    let bytes = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, bytes.to_vec())
}

fn bank(bytes: &[u8]) -> cratestack_schema::MapRpcBank {
    CborCodec.decode(bytes).expect("a MapRpcBank")
}

#[tokio::test]
async fn rpc_ops_keep_their_ids_and_run_against_the_mapped_names() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    for sql in [
        "DROP TABLE IF EXISTS map_rpc_bank",
        "CREATE TABLE map_rpc_bank (bank_id BIGINT PRIMARY KEY, bank_code TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL, region TEXT NOT NULL)",
    ] {
        cratestack::sqlx::query(sql).execute(pool).await.expect(sql);
    }
    let router = cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        NoProcedures,
        (),
        CborCodec,
        AlwaysAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    for (id, code, name) in [(1, "AAA", "Alpha"), (2, "BBB", "Beta"), (3, "CCC", "Gamma")] {
        let frame = json!({"id": id, "code": code, "displayName": name, "region": "eu"});
        let (status, body) = post(&router, "model.MapRpcBank.create", frame).await;
        assert!(status.is_success(), "{}", String::from_utf8_lossy(&body));
    }
    let (status, _) = post(
        &router,
        "model.MapRpcBank.create",
        json!({"id": 9, "code": "AAA", "displayName": "dup", "region": "eu"}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "duplicate on the mapped unique column"
    );

    let (status, body) = post(&router, "model.MapRpcBank.get", json!({"id": 2})).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(bank(&body).displayName, "Beta");

    let (status, body) = post(
        &router,
        "model.MapRpcBank.update",
        json!({"id": 2, "patch": {"displayName": "Beta 2", "code": "BB2"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(bank(&body).code, "BB2");

    // list: sort on a mapped field, then the cursor walk.
    let mut seen = Vec::new();
    let mut cursor = String::new();
    loop {
        let input = cratestack::rpc::RpcListInput {
            limit: Some(2),
            cursor: Some(cursor.clone()),
            sort: Some("-displayName".to_owned()),
            ..Default::default()
        };
        let (status, body) = post(
            &router,
            "model.MapRpcBank.list",
            serde_json::to_value(&input).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let page: cratestack::Page<cratestack_schema::MapRpcBank> =
            CborCodec.decode(&body).expect("page");
        seen.extend(page.items.iter().map(|b| b.id));
        match page.page_info.next_cursor {
            Some(next) => cursor = next,
            None => break,
        }
    }
    assert_eq!(seen, [3, 2, 1], "displayName DESC across two pages");

    let (status, _) = post(&router, "model.MapRpcBank.delete", json!({"id": 3})).await;
    assert!(status.is_success());
    let stored: i64 = cratestack::sqlx::query_scalar("SELECT count(*) FROM map_rpc_bank")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(stored, 2);
    let (status, _) = post(&router, "model.MapRpcBank.get", json!({"id": 3})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
