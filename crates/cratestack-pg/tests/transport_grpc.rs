//! `transport grpc` server codegen smoke test (ticket #171). Gated on the
//! `grpc` Cargo feature — without it, `include_server_schema!` against a
//! `transport grpc` schema is a `compile_error!` by design (see
//! `crates/cratestack-macros/src/include/reject_grpc.rs`), so this whole
//! file is skipped rather than failing a default `cargo test -p
//! cratestack-pg` run. Run with `cargo test -p cratestack-pg --features
//! grpc --test transport_grpc`.
//!
//! Uses `connect_lazy` (no live Postgres needed) — this test's job is
//! proving the *generated code compiles and mounts*, i.e. that the tonic
//! service, the pb mirror structs, and `into_router` all typecheck against
//! a real schema + committed `.pb.lock`. Dispatch-level (DB-backed)
//! coverage lives in `just test-pg`'s `banking_*`/`generated_client_rust`
//! style tests, which this fixture doesn't yet have a gRPC-client
//! counterpart for — see this ticket's final report for what's covered
//! vs. not.

#![cfg(feature = "grpc")]

use cratestack::CodecSet;
use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;

include_server_schema!("tests/fixtures/transport_grpc.cstack", db = Postgres);

fn test_db() -> cratestack_schema::Cratestack {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    cratestack_schema::Cratestack::builder(pool).build()
}

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

/// Proves `cratestack_schema::grpc::pb` exists with the expected mirror
/// types and `From`/`TryFrom` conversions actually typecheck against the
/// domain structs.
#[test]
fn pb_mirror_round_trips_widget() {
    let domain = cratestack_schema::Widget {
        id: 1,
        name: "Alpha".to_owned(),
    };
    let mirror = cratestack_schema::grpc::pb::Widget::from(&domain);
    assert_eq!(mirror.id, Some(1));
    assert_eq!(mirror.name, Some("Alpha".to_owned()));

    let back = cratestack_schema::Widget::try_from(mirror).expect("round trip should succeed");
    assert_eq!(back, domain);
}

/// Proves the tonic service actually mounts into an `axum::Router` —
/// `docs/design/protobuf.md` §7.2's axum/tonic alignment claim, exercised
/// for real rather than just asserted from `cargo tree` output.
#[tokio::test]
async fn grpc_service_mounts_into_axum_router() {
    let db = test_db();
    let codec = CodecSet::new(CborCodec, JsonCodec);
    let state = cratestack_schema::axum::ModelRouterState {
        db,
        codec,
        auth_provider: AllowAllAuth,
    };
    let _router: cratestack::axum::Router = cratestack_schema::grpc::into_router(state);
}
