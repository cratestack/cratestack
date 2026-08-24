//! `transport rpc` counterpart to `computed_fields_self_client.rs` —
//! proves the RPC self-client's decode targets
//! (`crates/cratestack-macros/src/client/rpc.rs`,
//! `crates/cratestack-macros/src/client/rpc/model.rs`) got the same
//! fix as the REST ones, for a model GET (`BatchableCall<C,
//! super::wire::<Model>>`) and a procedure returning a computed-bearing
//! `type` nested through another computed-bearing `type`.
//!
//! PG-gated: skips silently without `CRATESTACK_TEST_DATABASE_URL` /
//! `CRATESTACK_USE_TESTCONTAINERS` (`tests/support/pg.rs`).

use std::net::SocketAddr;

use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};

include_server_schema!(
    "tests/fixtures/computed_fields_self_client_rpc.cstack",
    db = Postgres
);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS self_client_rpc_photos")
        .execute(pool)
        .await
        .expect("drop table");
    query(
        "CREATE TABLE self_client_rpc_photos (
            id BIGINT PRIMARY KEY,
            storage_key TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create self_client_rpc_photos");
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    query("INSERT INTO self_client_rpc_photos (id, storage_key) VALUES (1, 'media/rpc.png')")
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
    fn resolve_self_client_rpc_photo_proxy_url(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::SelfClientRpcPhoto,
        params: Option<&cratestack_schema::SelfClientRpcProxyParams>,
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

    fn resolve_self_client_rpc_image_badge(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::SelfClientRpcImage,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<String, CratestackError>> + Send {
        let storage_key = source.storageKey.clone();
        async move { Ok(format!("badge-for-{storage_key}")) }
    }
}

#[derive(Clone)]
struct TestProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for TestProcedures {
    async fn get_self_client_rpc_card(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::get_self_client_rpc_card::Args,
        _authorized: cratestack_schema::procedures::get_self_client_rpc_card::Authorized,
    ) -> Result<
        cratestack_schema::procedures::get_self_client_rpc_card::Output,
        cratestack::CratestackError,
    > {
        Ok(cratestack_schema::SelfClientRpcCard {
            cover: cratestack_schema::SelfClientRpcImage {
                storageKey: args.storageKey,
            },
        })
    }
}

async fn spawn_server(pool: cratestack::sqlx::PgPool) -> (url::Url, tokio::task::JoinHandle<()>) {
    let db = cratestack_schema::Cratestack::builder(pool).build();
    let router = cratestack_schema::axum::rpc_router(
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
async fn rpc_self_client_model_get_includes_the_resolved_computed_field() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;
    seed(&test_pg.pool).await;

    let (base_url, _server) = spawn_server(test_pg.pool.clone()).await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    // `BatchableCall<C, wire::SelfClientRpcPhoto>` — NOT `models::
    // SelfClientRpcPhoto`, which has no `proxyUrl` field at all.
    let photo = client
        .self_client_rpc_photos()
        .get(&1, None)
        .await
        .expect("get should succeed");

    assert_eq!(photo.storageKey, "media/rpc.png");
    assert_eq!(photo.proxyUrl, "https://cdn.example/media/rpc.png");
}

/// RPC counterpart to
/// `computed_fields_self_client.rs`'s typed-`computedParams` proof:
/// `RpcGetInput { id, computed_params }` (`crates/cratestack-macros/src/
/// client/rpc/model.rs`) must carry the typed struct's encoded value all
/// the way through the RPC dispatcher's `parse_model_fetch_query` reuse
/// (`docs/design/computed-fields.md`'s "Parameterized resolvers on the
/// wire" section) to the resolver.
#[tokio::test]
async fn rpc_self_client_get_with_typed_computed_params_changes_the_resolved_value() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;
    seed(&test_pg.pool).await;

    let (base_url, _server) = spawn_server(test_pg.pool.clone()).await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    let params = cratestack_schema::client::SelfClientRpcPhotoComputedParams {
        proxyUrl: Some(cratestack_schema::SelfClientRpcProxyParams { width: Some(800) }),
    };
    let photo = client
        .self_client_rpc_photos()
        .get(&1, Some(&params))
        .await
        .expect("get should succeed");

    assert_eq!(
        photo.proxyUrl, "https://cdn.example/media/rpc.png?w=800",
        "typed computedParams must reach the resolver's `width` argument over RPC"
    );
}

/// `Default::default()` (every field `None`) must resolve to the
/// unparameterized value over RPC, same as REST.
#[tokio::test]
async fn rpc_self_client_get_with_default_computed_params_uses_the_unparameterized_value() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;
    seed(&test_pg.pool).await;

    let (base_url, _server) = spawn_server(test_pg.pool.clone()).await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    let params = cratestack_schema::client::SelfClientRpcPhotoComputedParams::default();
    let photo = client
        .self_client_rpc_photos()
        .get(&1, Some(&params))
        .await
        .expect("get should succeed");

    assert_eq!(photo.proxyUrl, "https://cdn.example/media/rpc.png");
}

/// `list`'s typed `computedParams` parameter over RPC — clones the
/// caller's `RpcListInput` and overwrites its `computed_params` field
/// with the typed struct's encoded value
/// (`crates/cratestack-macros/src/client/rpc/model.rs`).
#[tokio::test]
async fn rpc_self_client_list_with_typed_computed_params_changes_the_resolved_value() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;
    seed(&test_pg.pool).await;

    let (base_url, _server) = spawn_server(test_pg.pool.clone()).await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    let params = cratestack_schema::client::SelfClientRpcPhotoComputedParams {
        proxyUrl: Some(cratestack_schema::SelfClientRpcProxyParams { width: Some(800) }),
    };
    let photos = client
        .self_client_rpc_photos()
        .list(&cratestack::rpc::RpcListInput::default(), Some(&params))
        .await
        .expect("list should succeed");

    assert_eq!(photos.len(), 1);
    assert_eq!(
        photos[0].proxyUrl,
        "https://cdn.example/media/rpc.png?w=800"
    );
}

#[tokio::test]
async fn rpc_self_client_procedure_output_includes_the_nested_computed_field() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let (base_url, _server) = spawn_server(test_pg.pool.clone()).await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    let card = client
        .procedures()
        .get_self_client_rpc_card(
            &cratestack_schema::procedures::get_self_client_rpc_card::Args {
                storageKey: "media/rpc-two.png".to_owned(),
            },
        )
        .await
        .expect("procedure call should succeed");

    assert_eq!(card.cover.storageKey, "media/rpc-two.png");
    assert_eq!(card.cover.badge, "badge-for-media/rpc-two.png");
}
