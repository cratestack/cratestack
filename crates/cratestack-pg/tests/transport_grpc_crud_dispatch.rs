//! DB-backed regression coverage for the gRPC CRUD arm builders
//! (cratestack#524, refs #426): `crud_arms.rs`/`crud_arm_list.rs` dedup'd
//! five near-identical match arms (`get`/`delete`/`create`/`update`/
//! `list`) into one shared [`crud_arm_spec::build_unary_arm`] helper,
//! with the acceptance bar "generated output byte-identical before and
//! after" verified once by hand (`cargo expand` + sha256 diff) and never
//! checked in. `transport_grpc.rs` proves the generated router
//! compiles/mounts against a `connect_lazy` pool but never dispatches a
//! CRUD call through it, so a mistake in the dedup — e.g. an `ArmSpec`
//! wired to the wrong dispatch fn — would pass every existing CI job.
//!
//! This file closes that gap by driving all five arms through the real
//! generated `into_router()` against a real Postgres, asserting on both
//! the gRPC response *and* the underlying table so a wrong-dispatch bug
//! is visible even when the two dispatch fns involved return
//! superficially similar-shaped responses (e.g. `get` and `delete` both
//! return the plain `Gadget` message).
//!
//! Run with:
//! ```text
//! cargo test -p cratestack-pg --features grpc --test transport_grpc_crud_dispatch
//! ```
//! Skips silently unless `CRATESTACK_TEST_DATABASE_URL` or
//! `CRATESTACK_USE_TESTCONTAINERS` is set (see `tests/support/pg.rs`);
//! CI's `tests-db` job sets `CRATESTACK_REQUIRE_DB=1` so a skip there
//! panics instead of silently passing.

#![cfg(feature = "grpc")]

mod support;

use std::collections::HashMap;

use cratestack::sqlx::{Row, query};
use cratestack::{CodecSet, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_grpc::{frame_grpc_message, strip_grpc_frame};
use prost::Message as _;
use support::pg;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/transport_grpc_crud.cstack", db = Postgres);

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS gadgets")
        .execute(pool)
        .await
        .expect("drop gadgets table");
    query("CREATE TABLE gadgets (id BIGINT PRIMARY KEY, name TEXT NOT NULL)")
        .execute(pool)
        .await
        .expect("create gadgets table");
}

async fn seed_gadget(pool: &cratestack::sqlx::PgPool, id: i64, name: &str) {
    query("INSERT INTO gadgets (id, name) VALUES ($1, $2)")
        .bind(id)
        .bind(name)
        .execute(pool)
        .await
        .expect("seed gadget");
}

async fn gadget_row(pool: &cratestack::sqlx::PgPool, id: i64) -> Option<String> {
    query("SELECT name FROM gadgets WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .expect("query gadget row")
        .map(|row| row.get::<String, _>("name"))
}

async fn gadget_count(pool: &cratestack::sqlx::PgPool) -> i64 {
    query("SELECT COUNT(*) FROM gadgets")
        .fetch_one(pool)
        .await
        .expect("count gadgets")
        .get(0)
}

/// Satisfies `auth() != null` on every `Gadget` `@@allow` rule — this
/// fixture's own policy depth isn't the point here (`transport_grpc.rs`
/// already exists for that); this file's job is arm dispatch.
#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = cratestack::CoolError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl std::future::Future<Output = Result<cratestack::CoolContext, Self::Error>> + Send
    {
        std::future::ready(Ok(cratestack::CoolContext::authenticated([])))
    }
}

/// `transport_grpc_crud.cstack` declares zero procedures — this fixture's
/// only job is CRUD arm dispatch — so `ProcedureRegistry` is an empty
/// trait here (see `cratestack-macros/src/include/server.rs`'s
/// `#(#procedure_registry_methods)*`) and `Procedures` only exists to
/// satisfy `into_router`'s generic bound.
#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {}

fn router(pool: cratestack::sqlx::PgPool) -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder(pool).build();
    let codec = CodecSet::new(CborCodec, JsonCodec);
    cratestack_schema::grpc::into_router(db, Procedures, codec, AllowAllAuth)
}

/// Sends one unary gRPC call and returns the raw decoded response bytes
/// alongside the HTTP status — callers pick the pb type to decode with,
/// since each arm's response message differs.
async fn call(
    router: &cratestack::axum::Router,
    path: &str,
    encoded: Vec<u8>,
) -> (cratestack::axum::http::StatusCode, Vec<u8>) {
    let framed = frame_grpc_message(&encoded, false);
    let request = cratestack::axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/grpc")
        .version(cratestack::axum::http::Version::HTTP_2)
        .body(cratestack::axum::body::Body::from(framed))
        .unwrap();

    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let unframed = strip_grpc_frame(&body)
        .expect("response must carry one gRPC message frame")
        .to_vec();
    (status, unframed)
}

// ───── create ─────────────────────────────────────────────────────────

/// The `create` arm: no seed data — the row must not exist before this
/// call and must exist, with the submitted values, after it. Proves
/// `build_create_arm`'s `ArmSpec` actually reached
/// `handle_create_gadgets_dispatch`, not some other verb's dispatch fn.
#[tokio::test]
async fn grpc_create_arm_persists_the_gadget_via_real_dispatch() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    let router = router(pool.clone());

    assert_eq!(
        gadget_row(pool, 1).await,
        None,
        "gadget 1 must not pre-exist"
    );

    let input = cratestack_schema::grpc::pb::CreateGadgetInput {
        id: Some(1),
        name: Some("Alpha".to_owned()),
    };
    let (status, body) = call(
        &router,
        "/gadgets_api.Api/ModelGadgetCreate",
        input.encode_to_vec(),
    )
    .await;
    assert_eq!(status, cratestack::axum::http::StatusCode::OK);
    let output = cratestack_schema::grpc::pb::Gadget::decode(body.as_slice())
        .expect("response frame must decode as Gadget");
    assert_eq!(output.id, Some(1));
    assert_eq!(output.name.as_deref(), Some("Alpha"));

    assert_eq!(
        gadget_row(pool, 1).await,
        Some("Alpha".to_owned()),
        "create dispatch must have actually persisted the row"
    );
}

// ───── get ──────────────────────────────────────────────────────────

/// The `get` arm: seeded directly via SQL (independent of the `create`
/// arm), so this test's assertion isolates `get`'s own dispatch wiring.
/// Also asserts the row is UNCHANGED afterward — the decisive check that
/// catches a `get`<->`delete` dispatch swap, since a swapped `get` arm
/// would silently delete the row while still returning a Gadget-shaped
/// response (both arms share the same request/response pb types).
#[tokio::test]
async fn grpc_get_arm_reads_the_persisted_gadget_via_real_dispatch() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed_gadget(pool, 7, "Gizmo").await;
    let router = router(pool.clone());

    let input = cratestack_schema::grpc::pb::GadgetRpcPkInput { id: Some(7) };
    let (status, body) = call(
        &router,
        "/gadgets_api.Api/ModelGadgetGet",
        input.encode_to_vec(),
    )
    .await;
    assert_eq!(status, cratestack::axum::http::StatusCode::OK);
    let output = cratestack_schema::grpc::pb::Gadget::decode(body.as_slice())
        .expect("response frame must decode as Gadget");
    assert_eq!(output.id, Some(7));
    assert_eq!(output.name.as_deref(), Some("Gizmo"));

    assert_eq!(
        gadget_row(pool, 7).await,
        Some("Gizmo".to_owned()),
        "get dispatch must be read-only — the row must still be present and unchanged"
    );
}

// ───── list ─────────────────────────────────────────────────────────

/// The `list` arm: two seeded rows, both must come back. Proves
/// `build_list_arm`'s dispatch fn (and its unpaged `Vec<Gadget>` ->
/// `PageOf<Gadget>` wrap, since this fixture's `Gadget` has no `@@paged`)
/// actually ran.
#[tokio::test]
async fn grpc_list_arm_returns_the_persisted_gadgets_via_real_dispatch() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed_gadget(pool, 1, "Alpha").await;
    seed_gadget(pool, 2, "Beta").await;
    let router = router(pool.clone());

    let input = cratestack_schema::grpc::pb::GadgetRpcListInput {
        limit: None,
        offset: None,
        fields: vec![],
        include: vec![],
        include_fields: HashMap::new(),
        sort: Some("id".to_owned()),
        where_expr: None,
        or: None,
        filters: vec![],
    };
    let (status, body) = call(
        &router,
        "/gadgets_api.Api/ModelGadgetList",
        input.encode_to_vec(),
    )
    .await;
    assert_eq!(status, cratestack::axum::http::StatusCode::OK);
    let output = cratestack_schema::grpc::pb::PageOfGadget::decode(body.as_slice())
        .expect("response frame must decode as PageOfGadget");

    let mut ids_and_names: Vec<(Option<i64>, Option<String>)> = output
        .items
        .into_iter()
        .map(|item| (item.id, item.name))
        .collect();
    ids_and_names.sort_by_key(|(id, _)| *id);
    assert_eq!(
        ids_and_names,
        vec![
            (Some(1), Some("Alpha".to_owned())),
            (Some(2), Some("Beta".to_owned())),
        ]
    );
}

// ───── update ───────────────────────────────────────────────────────

/// The `update` arm: seeded via SQL, patched via gRPC, re-read via SQL.
/// Proves `build_update_arm`'s `into_id_and_patch()` -> re-encode ->
/// `handle_update_gadget_dispatch` chain actually mutates the row.
#[tokio::test]
async fn grpc_update_arm_mutates_the_persisted_gadget_via_real_dispatch() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed_gadget(pool, 3, "Old-Name").await;
    let router = router(pool.clone());

    let input = cratestack_schema::grpc::pb::GadgetRpcUpdateInput {
        id: Some(3),
        patch: Some(Box::new(cratestack_schema::grpc::pb::UpdateGadgetInput {
            name: Some("New-Name".to_owned()),
        })),
    };
    let (status, body) = call(
        &router,
        "/gadgets_api.Api/ModelGadgetUpdate",
        input.encode_to_vec(),
    )
    .await;
    assert_eq!(status, cratestack::axum::http::StatusCode::OK);
    let output = cratestack_schema::grpc::pb::Gadget::decode(body.as_slice())
        .expect("response frame must decode as Gadget");
    assert_eq!(output.id, Some(3));
    assert_eq!(output.name.as_deref(), Some("New-Name"));

    assert_eq!(
        gadget_row(pool, 3).await,
        Some("New-Name".to_owned()),
        "update dispatch must have actually mutated the row"
    );
}

// ───── delete ───────────────────────────────────────────────────────

/// The `delete` arm: seeded via SQL, deleted via gRPC, absence confirmed
/// via SQL. The decisive check is the row COUNT afterward — the
/// counterpart to `get`'s "row unchanged" assertion above, catching a
/// `get`<->`delete` swap from the other direction: a swapped `delete`
/// arm would return a superficially valid Gadget response (both arms
/// share request/response pb types) while never actually deleting.
#[tokio::test]
async fn grpc_delete_arm_removes_the_persisted_gadget_via_real_dispatch() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed_gadget(pool, 9, "Doomed").await;
    let router = router(pool.clone());

    assert_eq!(gadget_count(pool).await, 1);

    let input = cratestack_schema::grpc::pb::GadgetRpcPkInput { id: Some(9) };
    let (status, body) = call(
        &router,
        "/gadgets_api.Api/ModelGadgetDelete",
        input.encode_to_vec(),
    )
    .await;
    assert_eq!(status, cratestack::axum::http::StatusCode::OK);
    let output = cratestack_schema::grpc::pb::Gadget::decode(body.as_slice())
        .expect("response frame must decode as Gadget");
    assert_eq!(output.id, Some(9));
    assert_eq!(output.name.as_deref(), Some("Doomed"));

    assert_eq!(
        gadget_count(pool).await,
        0,
        "delete dispatch must have actually removed the row"
    );
    assert_eq!(gadget_row(pool, 9).await, None);
}
