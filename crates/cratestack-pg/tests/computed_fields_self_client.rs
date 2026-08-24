//! Decisive end-to-end proof for the bug documented in
//! `docs/design/computed-fields.md`'s "Exclusions" section and closed by
//! this change: the server's embedded self/peer-calling client
//! (`include_server_schema!`'s `cratestack_schema::client::Client`) used
//! to decode every response into the server-side struct
//! (`super::models::<Model>`/`super::types::<Type>`, which excludes
//! `@computed` fields by design), silently dropping every resolved
//! computed value. Every other computed-fields test in this crate proves
//! the *router* resolves and encodes computed fields correctly
//! (`computed_fields_router.rs`, `computed_fields_rpc.rs`) — none of them
//! go through the self-client's own decode step, so none of them would
//! have caught this. This file does: it spins up the real generated
//! router against real Postgres, then reaches it exclusively through
//! `cratestack_schema::client::Client` over a real HTTP round trip
//! (`tokio::net::TcpListener` + `axum::serve`, same harness as
//! `generated_client_rust.rs`), and asserts the decoded Rust struct
//! actually carries the resolved value.
//!
//! PG-gated: skips silently without `CRATESTACK_TEST_DATABASE_URL` /
//! `CRATESTACK_USE_TESTCONTAINERS` (`tests/support/pg.rs`).
//!
//! Covers:
//! - A model `GET` through the self-client: `SelfClientPhoto.proxyUrl` is
//!   present on the decoded `cratestack_schema::client::Client` result.
//! - A procedure returning a computed-bearing `type` nested through
//!   *another* computed-bearing `type` (`SelfClientCard.cover` is a
//!   `SelfClientImage`, itself computed-bearing) — proves the recursive
//!   wire-struct substitution end to end, not just at the token level
//!   (`crates/cratestack-macros/src/computed/wire/tests.rs` covers that
//!   separately, DB-lessly).

use std::net::SocketAddr;

use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};

include_server_schema!(
    "tests/fixtures/computed_fields_self_client.cstack",
    db = Postgres
);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS self_client_photos")
        .execute(pool)
        .await
        .expect("drop table");
    query(
        "CREATE TABLE self_client_photos (
            id BIGINT PRIMARY KEY,
            storage_key TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create self_client_photos");
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    query("INSERT INTO self_client_photos (id, storage_key) VALUES (1, 'media/one.png')")
        .execute(pool)
        .await
        .expect("seed photo");
}

#[derive(Clone)]
struct PassThroughAuth;

impl AuthProvider for PassThroughAuth {
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
struct TestResolver;

impl cratestack_schema::ComputedFieldResolver for TestResolver {
    fn resolve_self_client_photo_proxy_url(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::SelfClientPhoto,
        params: Option<&cratestack_schema::SelfClientProxyParams>,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<String, CratestackError>> + Send {
        let storage_key = source.storageKey.clone();
        let width = params.and_then(|p| p.width);
        async move {
            Ok(match width {
                Some(width) => format!("https://cdn.example/{storage_key}?w={width}"),
                None => format!("https://cdn.example/{storage_key}"),
            })
        }
    }

    fn resolve_self_client_image_badge(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::SelfClientImage,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<String, CratestackError>> + Send {
        let storage_key = source.storageKey.clone();
        async move { Ok(format!("badge-for-{storage_key}")) }
    }
}

#[derive(Clone)]
struct TestProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for TestProcedures {
    async fn get_self_client_card(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::get_self_client_card::Args,
        _authorized: cratestack_schema::procedures::get_self_client_card::Authorized,
    ) -> Result<
        cratestack_schema::procedures::get_self_client_card::Output,
        cratestack::CratestackError,
    > {
        Ok(cratestack_schema::SelfClientCard {
            cover: cratestack_schema::SelfClientImage {
                storageKey: args.storageKey,
            },
        })
    }
}

async fn spawn_server(pool: cratestack::sqlx::PgPool) -> (url::Url, tokio::task::JoinHandle<()>) {
    let db = cratestack_schema::Cratestack::builder(pool).build();
    let router = cratestack_schema::axum::router(
        db,
        TestProcedures,
        TestResolver,
        CborCodec,
        PassThroughAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr: SocketAddr = listener.local_addr().expect("listener should have addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("server should run");
    });

    (
        url::Url::parse(&format!("http://{addr}")).expect("base url should parse"),
        handle,
    )
}

#[tokio::test]
async fn self_client_model_get_includes_the_resolved_computed_field() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;
    seed(&test_pg.pool).await;

    let (base_url, _server) = spawn_server(test_pg.pool.clone()).await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    // Decodes into `cratestack_schema::client::wire::SelfClientPhoto`
    // (the wire-shape mirror), NOT `cratestack_schema::models::
    // SelfClientPhoto` (the server-side struct, which has no `proxyUrl`
    // field at all) — this is the load-bearing type-level proof that the
    // self-client's decode target actually changed.
    let photo = client
        .self_client_photos()
        .get(&1, None, &[])
        .await
        .expect("get should succeed");

    assert_eq!(photo.storageKey, "media/one.png");
    assert_eq!(photo.proxyUrl, "https://cdn.example/media/one.png");
}

/// Decisive proof for the typed Rust client's `computedParams` surface
/// (`crates/cratestack-macros/src/client/computed_params.rs`,
/// `docs/design/computed-fields.md`'s "Downstream" section): passing a
/// `SelfClientPhotoComputedParams { proxyUrl: Some(..width 800..) }`
/// through `get` must change the resolved value the same way the raw
/// `?computedParams=` query parameter does in `computed_fields_router.rs`
/// — proving the typed struct's `to_query_value()` actually reaches the
/// server, not just that it compiles.
#[tokio::test]
async fn self_client_get_with_typed_computed_params_changes_the_resolved_value() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;
    seed(&test_pg.pool).await;

    let (base_url, _server) = spawn_server(test_pg.pool.clone()).await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    let params = cratestack_schema::client::SelfClientPhotoComputedParams {
        proxyUrl: Some(cratestack_schema::SelfClientProxyParams { width: Some(800) }),
    };
    let photo = client
        .self_client_photos()
        .get(&1, Some(&params), &[])
        .await
        .expect("get should succeed");

    assert_eq!(
        photo.proxyUrl, "https://cdn.example/media/one.png?w=800",
        "typed computedParams must reach the resolver's `width` argument"
    );
}

/// `Default::default()` (every field `None`) must resolve to the
/// unparameterized value — `to_query_value()` returning `None` in that
/// case is what keeps a caller who never touches `computedParams` from
/// silently sending an empty-but-present `?computedParams={}`.
#[tokio::test]
async fn self_client_get_with_default_computed_params_uses_the_unparameterized_value() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;
    seed(&test_pg.pool).await;

    let (base_url, _server) = spawn_server(test_pg.pool.clone()).await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    let params = cratestack_schema::client::SelfClientPhotoComputedParams::default();
    let photo = client
        .self_client_photos()
        .get(&1, Some(&params), &[])
        .await
        .expect("get should succeed");

    assert_eq!(photo.proxyUrl, "https://cdn.example/media/one.png");
}

/// `list`'s typed `computedParams` parameter — same wiring as `get`,
/// exercised through the list route instead.
#[tokio::test]
async fn self_client_list_with_typed_computed_params_changes_the_resolved_value() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;
    seed(&test_pg.pool).await;

    let (base_url, _server) = spawn_server(test_pg.pool.clone()).await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    let params = cratestack_schema::client::SelfClientPhotoComputedParams {
        proxyUrl: Some(cratestack_schema::SelfClientProxyParams { width: Some(800) }),
    };
    let photos = client
        .self_client_photos()
        .list(&[], Some(&params), &[])
        .await
        .expect("list should succeed");

    assert_eq!(photos.len(), 1);
    assert_eq!(
        photos[0].proxyUrl,
        "https://cdn.example/media/one.png?w=800"
    );
}

#[tokio::test]
async fn self_client_procedure_output_includes_the_nested_computed_field() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let (base_url, _server) = spawn_server(test_pg.pool.clone()).await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    // `SelfClientCard` is computed-bearing only because it nests
    // `SelfClientImage`, itself computed-bearing — proves the recursive
    // wire-struct substitution (`wire::SelfClientCard.cover:
    // wire::SelfClientImage`, not `wire::SelfClientCard.cover:
    // models::SelfClientImage`) actually round-trips over real HTTP, not
    // just at the token level.
    let card = client
        .procedures()
        .get_self_client_card(
            &cratestack_schema::procedures::get_self_client_card::Args {
                storageKey: "media/two.png".to_owned(),
            },
            &[],
        )
        .await
        .expect("procedure call should succeed");

    assert_eq!(card.cover.storageKey, "media/two.png");
    assert_eq!(card.cover.badge, "badge-for-media/two.png");
}
