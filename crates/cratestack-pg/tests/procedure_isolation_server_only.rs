//! An `@isolation` procedure's output never carries a `@server_only` value
//! (GHSA-r67q-4qqq-g9gm × GHSA-ch54).
//!
//! An `@isolation` procedure composes its `@computed` output inside its
//! attempt, through `isolated_compose_tokens` rather than the dispatch tail
//! every other procedure uses (docs/design/procedure-isolation.md §6). The
//! model's value is then built field by field by `compose_<owner>_value`,
//! which bypasses the struct's `#[serde(skip)]`, so it must use the
//! `@server_only`-free field list on that path too. The resolver reports the
//! transaction level it ran at, which proves the output really went through
//! the attempt (and the control, without `@isolation`, did not).
//!
//! REST, RPC unary and `/rpc/batch`, unary and `T[]` outputs. MCP:
//! `procedure_isolation_mcp.rs`. Needs a database: `just test-ci-db --test
//! procedure_isolation_server_only -- --test-threads=1` with
//! `CRATESTACK_REQUIRE_DB=1`.

#![cfg(feature = "codec-json")]

mod support;

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::extract::ConnectInfo;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use support::pg;
use tower::util::ServiceExt;

const SECRET: &str = "HUNTER2-server-only";

macro_rules! vault_impl {
    () => {
        use super::SECRET;
        use cratestack::sqlx;
        use cratestack::{CratestackContext, CratestackError};
        use cratestack_schema::procedures as p;
        use cratestack_schema::{Cratestack, IsoVault, IsolatedCratestack};

        #[derive(Clone)]
        pub struct Procedures;

        #[derive(Clone)]
        pub struct Resolvers;

        fn vault(id: i64) -> IsoVault {
            IsoVault {
                id,
                label: format!("vault-{id}"),
                secret: SECRET.to_owned(),
            }
        }

        impl p::ProcedureRegistry for Procedures {
            async fn open_vault(
                &self,
                _db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                _args: p::open_vault::Args,
                _authorized: p::open_vault::Authorized,
            ) -> Result<IsoVault, CratestackError> {
                Ok(vault(1))
            }

            async fn open_vaults(
                &self,
                _db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                _args: p::open_vaults::Args,
                _authorized: p::open_vaults::Authorized,
            ) -> Result<Vec<IsoVault>, CratestackError> {
                Ok(vec![vault(1), vault(2)])
            }

            async fn open_vault_plain(
                &self,
                _db: &Cratestack,
                _ctx: &CratestackContext,
                _args: p::open_vault_plain::Args,
                _authorized: p::open_vault_plain::Authorized,
            ) -> Result<IsoVault, CratestackError> {
                Ok(vault(1))
            }
        }

        impl cratestack_schema::ComputedFieldResolver for Resolvers {
            /// `<transaction level the resolver ran at>/<secret length>`:
            /// derived from the `@server_only` value without being it.
            fn resolve_iso_vault_hint(
                &self,
                db: &Cratestack,
                source: &IsoVault,
                _ctx: &CratestackContext,
            ) -> impl core::future::Future<Output = Result<String, CratestackError>> + Send {
                let (db, length) = (db.clone(), source.secret.len());
                async move {
                    let level = db
                        .transaction(async |tx| {
                            sqlx::query_scalar::<_, String>(
                                "SELECT current_setting('transaction_isolation')",
                            )
                            .fetch_one(&mut ***tx)
                            .await
                            .map_err(cratestack::cratestack_error_from_sqlx)
                        })
                        .await?;
                    Ok(format!("{level}/{length}"))
                }
            }
        }
    };
}

pub mod rest {
    use cratestack::include_server_schema;
    include_server_schema!(
        "tests/fixtures/procedure_isolation_server_only.cstack",
        db = Postgres
    );
    vault_impl!();
}

pub mod rpc {
    use cratestack::include_server_schema;
    include_server_schema!(
        "tests/fixtures/procedure_isolation_server_only_rpc.cstack",
        db = Postgres
    );
    vault_impl!();
}

#[derive(Clone)]
struct AlwaysAuth;

impl AuthProvider for AlwaysAuth {
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

async fn post(router: &cratestack::axum::Router, uri: &str, body: &str) -> (StatusCode, String) {
    let mut request = Request::post(uri)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap();
    let peer: std::net::SocketAddr = "192.0.2.94:1".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// The body must carry the label and the derived hint, and nothing of the
/// `@server_only` field: neither its value nor its key.
fn assert_masked(what: &str, text: &str, level: &str) {
    assert!(!text.contains(SECRET), "{what} leaked the value: {text}");
    assert!(
        !text.contains("\"secret\""),
        "{what} leaked the key: {text}"
    );
    assert!(text.contains("\"label\":\"vault-1\""), "{what}: {text}");
    let hint = format!("\"hint\":\"{level}/{}\"", SECRET.len());
    assert!(text.contains(&hint), "{what}: expected {hint} in {text}");
}

const PROBE: &str = r#"{"args":{"nonce":"x"}}"#;

#[tokio::test]
async fn an_isolated_procedures_output_never_carries_a_server_only_value() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    let rest = rest::cratestack_schema::axum::router(
        rest::cratestack_schema::Cratestack::builder(pool.clone()).build(),
        rest::Procedures,
        rest::Resolvers,
        JsonCodec,
        AlwaysAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let rpc = rpc::cratestack_schema::axum::rpc_router(
        rpc::cratestack_schema::Cratestack::builder(pool.clone()).build(),
        rpc::Procedures,
        rpc::Resolvers,
        JsonCodec,
        AlwaysAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    let cases = [
        ("openVault", "serializable"),
        ("openVaults", "repeatable read"),
        ("openVaultPlain", "read committed"),
    ];
    for (procedure, level) in cases {
        for (label, router, uri) in [
            ("REST", &rest, format!("/$procs/{procedure}")),
            ("RPC", &rpc, format!("/rpc/procedure.{procedure}")),
        ] {
            let (status, text) = post(router, &uri, PROBE).await;
            println!("{label} {procedure}: {status} {text}");
            assert_eq!(status, StatusCode::OK, "{label} {procedure}: {text}");
            assert_masked(&format!("{label} {procedure}"), &text, level);
        }

        let batch = format!(r#"[{{"id":1,"op":"procedure.{procedure}","input":{PROBE}}}]"#);
        let (status, text) = post(&rpc, "/rpc/batch", &batch).await;
        println!("RPC batch {procedure}: {status} {text}");
        assert_eq!(status, StatusCode::OK, "batch {procedure}: {text}");
        assert_masked(&format!("batch {procedure}"), &text, level);
    }
}
