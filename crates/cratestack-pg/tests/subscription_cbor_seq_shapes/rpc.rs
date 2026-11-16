//! The RPC model feed (`GET /rpc/subscribe/model.SeqNote.subscribe`) over cbor-seq.

use std::time::Duration;

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode, header};
use cratestack::{CratestackContext, Value};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

use super::HeaderCaller;
use super::common::{TABLE, assert_one_event, frames};
use super::support::pg;

mod schema {
    use cratestack::include_server_schema;
    include_server_schema!(
        "tests/fixtures/subscription_cbor_seq_shapes_rpc.cstack",
        db = Postgres,
        decimal = RustDecimal
    );

    #[derive(Clone)]
    pub struct NoProcedures;

    impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}
}

use schema::cratestack_schema;

#[tokio::test]
async fn an_rpc_model_feed_carries_byte_string_uuids_and_exact_decimals() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    for statement in TABLE {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let router = cratestack_schema::axum::rpc_router(
        db.clone(),
        schema::NoProcedures,
        (),
        CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let request = Request::get("/rpc/subscribe/model.SeqNote.subscribe")
        .header("accept", "application/cbor-seq")
        .header("x-caller", "1")
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.expect("subscribe route");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/cbor-seq"
    );
    assert_eq!(response.headers()[header::VARY], "Accept");

    let row_id = uuid::Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
    db.seq_note()
        .bind(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )]))
        .create(cratestack_schema::CreateSeqNoteInput {
            id: row_id,
            amount: "12.50".parse().expect("a decimal"),
            body: "hello".to_owned(),
        })
        .run()
        .await
        .expect("note created");
    assert_one_event(&frames(response, Duration::from_millis(750)).await, row_id);
}
