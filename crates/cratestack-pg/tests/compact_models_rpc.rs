//! Compact wire keys for model ops over the real `transport rpc` router on
//! Postgres (`docs/design/compact-wire-keys.md`): a compact `create` frame is
//! opened, its reply and a `list` reply are integer-keyed, and the list
//! input (a framework frame) stays named. Skips without
//! `CRATESTACK_TEST_DATABASE_URL`, like the other PG-backed suites. The
//! negotiation rules themselves are covered without a database in
//! `cratestack-api/tests/compact_*.rs`.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::compact::{COMPACT_JSON, KeyTables};
use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{AuthProvider, ContractSelector, CratestackContext, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/enum_ordinals_rpc.cstack", db = Postgres);

mod support;
use support::pg;

const SCHEMA_SRC: &str = include_str!("fixtures/enum_ordinals_rpc.cstack");

#[derive(Clone)]
struct AnyoneAuth;

impl AuthProvider for AnyoneAuth {
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

async fn reset(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS rpc_payments")
        .execute(pool)
        .await
        .expect("drop rpc_payments");
    let empty = cratestack_parser::parse_schema("").expect("empty schema parses");
    let next = cratestack_parser::parse_schema(SCHEMA_SRC).expect("fixture parses");
    let up = cratestack_migrate::emit::postgres::emit(
        &cratestack_migrate::diff(&empty, &next).expect("diff"),
    )
    .up;
    // `AssertSqlSafe`: test-only DDL from the migrate emitter.
    cratestack::sqlx::raw_sql(cratestack::sqlx::AssertSqlSafe(up))
        .execute(pool)
        .await
        .expect("emitted DDL applies");
}

fn selector(op: &str) -> String {
    let digest = cratestack_schema::OP_CONTRACTS
        .iter()
        .find(|(key, _)| *key == op)
        .expect("op")
        .1;
    ContractSelector::of(&digest).to_header_value()
}

async fn rpc(
    router: &cratestack::axum::Router,
    op: &str,
    content_type: &str,
    body: String,
) -> (StatusCode, Option<String>, String) {
    let response = router
        .clone()
        .oneshot(
            Request::post(format!("/rpc/{op}"))
                .header("accept", COMPACT_JSON)
                .header("content-type", content_type)
                .header("cratestack-contract", selector(op))
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    let status = response.status();
    let kind = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        kind,
        String::from_utf8(bytes.to_vec()).expect("utf8"),
    )
}

#[tokio::test]
async fn model_ops_speak_compact_json_over_rpc() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    let router = cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build(),
        NoProcedures,
        (),
        JsonCodec,
        AnyoneAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    // The model's fields are numbered in name order: `id` is 0, `status` 1.
    let tables = KeyTables::from_bytes(cratestack_schema::KEY_TABLES).expect("tables");
    let fields: Vec<&str> = tables.types[0]
        .fields
        .iter()
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(fields, ["id", "status"]);

    let (code, kind, body) = rpc(
        &router,
        "model.RpcPayment.create",
        COMPACT_JSON,
        r#"{"0":1,"1":2}"#.to_owned(),
    )
    .await;
    // A create answers 201 on RPC as on REST (`bigint_end_to_end_rpc.rs`).
    assert_eq!(code, StatusCode::CREATED, "{body}");
    assert_eq!(kind.as_deref(), Some(COMPACT_JSON));
    assert_eq!(body, r#"{"0":1,"1":2}"#);

    // The list input is a framework frame and stays named; the rows do not.
    let (code, kind, body) = rpc(
        &router,
        "model.RpcPayment.list",
        "application/json",
        r#"{"sort":"id"}"#.to_owned(),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(kind.as_deref(), Some(COMPACT_JSON));
    assert_eq!(body, r#"[{"0":1,"1":2}]"#);
}
