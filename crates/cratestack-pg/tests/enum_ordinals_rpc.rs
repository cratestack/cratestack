//! Ordinal enums (`Pending = 1`, `docs/design/enum-ordinals.md`) over the
//! RPC transport, the parity half of `tests/enum_ordinals.rs`: a create
//! frame carries the number (a name still decodes), the reply carries
//! the number, and a list filter carries the name.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::rpc::{RpcListInput, RpcListPredicate};
use cratestack::sqlx::query;
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
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

async fn rpc(router: &cratestack::axum::Router, op: &str, frame: Vec<u8>) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(
            Request::post(format!("/rpc/{op}"))
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .body(Body::from(frame))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
}

#[tokio::test]
async fn rpc_frames_carry_the_number_and_list_filters_the_name() {
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

    for (id, status) in [("1", "2"), ("2", r#""Approved""#), ("3", "5")] {
        let frame = format!(r#"{{"id":{id},"status":{status}}}"#).into_bytes();
        let (code, body) = rpc(&router, "model.RpcPayment.create", frame).await;
        assert!(code.is_success(), "create {id}: {code} {body}");
        // The `Refunded` row is created but its reply is unreadable.
        if id != "3" {
            assert!(body.contains(r#""status":2"#), "{body}");
        }
    }

    let list = |value: &str| {
        JsonCodec
            .encode(&RpcListInput {
                filters: vec![RpcListPredicate {
                    key: "status__in".to_owned(),
                    value: value.to_owned(),
                }],
                sort: Some("id".to_owned()),
                ..Default::default()
            })
            .expect("encode list input")
    };
    let (code, body) = rpc(&router, "model.RpcPayment.list", list("Approved,Refunded")).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let rows: Vec<serde_json::Value> = serde_json::from_str(&body).expect("json");
    // `Refunded` is hidden by `status != Refunded`, compared as 5 in SQL.
    assert_eq!(
        rows.iter().map(|r| r["id"].clone()).collect::<Vec<_>>(),
        [1, 2]
    );

    let (code, _) = rpc(&router, "model.RpcPayment.list", list("2")).await;
    assert_eq!(
        code,
        StatusCode::BAD_REQUEST,
        "a query value is a name (O2)"
    );
}
