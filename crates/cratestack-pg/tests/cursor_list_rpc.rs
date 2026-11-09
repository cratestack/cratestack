//! RPC parity for keyset pagination (cratestack#1304): `RpcListInput.cursor`
//! is synthesized into `?cursor=` and re-enters the one list dispatch REST
//! uses, so a walk over `model.RpcCursorNote.list` pages exactly like REST and a bad
//! cursor is refused the same way. PG-backed: skips without
//! `CRATESTACK_TEST_DATABASE_URL`.

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/cursor_list_rpc.cstack", db = Postgres);

mod support;

use support::pg;

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
            Value::Int(1),
        )])))
    }
}

#[derive(Clone)]
struct NoProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

async fn list(
    router: &cratestack::axum::Router,
    input: cratestack::rpc::RpcListInput,
) -> (
    StatusCode,
    Option<cratestack::Page<cratestack_schema::RpcCursorNote>>,
) {
    let codec = CborCodec;
    let response = router
        .clone()
        .oneshot(
            Request::post("/rpc/model.RpcCursorNote.list")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(codec.encode(&input).expect("encode")))
                .expect("request should build"),
        )
        .await
        .expect("rpc dispatch should complete");
    let status = response.status();
    let body = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        (status == StatusCode::OK).then(|| codec.decode(&body).expect("page")),
    )
}

#[tokio::test]
async fn rpc_cursor_walk_matches_rest_semantics_and_refuses_bad_cursors() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    for sql in [
        "DROP TABLE IF EXISTS rpc_cursor_notes",
        "CREATE TABLE rpc_cursor_notes (id BIGINT PRIMARY KEY, label TEXT NOT NULL, rank BIGINT, hidden BOOLEAN NOT NULL)",
        "INSERT INTO rpc_cursor_notes VALUES (1,'a',3,false),(2,'b',1,false),(3,'c',3,false),(4,'d',NULL,false),(5,'e',2,true)",
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
    let input = |cursor: &str, sort: &str| cratestack::rpc::RpcListInput {
        limit: Some(2),
        cursor: Some(cursor.to_owned()),
        sort: Some(sort.to_owned()),
        ..Default::default()
    };

    let mut seen = Vec::new();
    let mut cursor = String::new();
    loop {
        let (status, page) = list(&router, input(&cursor, "-rank")).await;
        assert_eq!(status, StatusCode::OK);
        let page = page.expect("page");
        seen.extend(page.items.iter().map(|note| note.id));
        match page.page_info.next_cursor {
            Some(next) => {
                assert!(page.page_info.has_next_page);
                cursor = next;
            }
            None => break,
        }
    }
    assert_eq!(
        seen,
        [1, 3, 2, 4],
        "rank DESC NULLS LAST, id tiebreak, hidden row 5 never served"
    );

    let (_, first) = list(&router, input("", "-rank")).await;
    let stale = first.unwrap().page_info.next_cursor.expect("cursor");
    for bad in [
        input(&stale, "rank"),
        input("not-a-cursor", "-rank"),
        cratestack::rpc::RpcListInput {
            offset: Some(1),
            ..input(&stale, "-rank")
        },
    ] {
        let (status, _) = list(&router, bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
