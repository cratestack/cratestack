//! A `@server_only` field is masked on the way *in*, not only on the way
//! out. The generated `Create*Input`/`Update*Input` structs leave such a
//! field out entirely, but a procedure argument can name a model directly
//! (or through a `type` that embeds one), and that decodes the full model
//! struct. The field used to carry `#[serde(skip_serializing, default)]`,
//! and `default` only applies when the key is *absent*: a client that sent
//! `"secret": "from-client"` had it deserialized and handed to the
//! procedure implementation as if the server had set it.
//!
//! Exercised end to end on both transports (REST `/$procs/<name>` and RPC
//! `/rpc/procedure.<name>`), with a raw JSON body, because the threat is a
//! hand-built request: the generated clients never serialize the field.
//! DB-less (`connect_lazy`): the procedures never touch Postgres.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::serde_json::{self, Value as Json, json};
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

#[derive(Clone)]
struct AllowAllAuth;

impl AuthProvider for AllowAllAuth {
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

fn lazy_pool() -> cratestack::sqlx::PgPool {
    PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse")
}

/// A model body carrying client-chosen values for both masked fields,
/// including a `Bytes` one (whose non-masked twin takes a custom
/// `deserialize_with`).
fn account_with_masked_values() -> Json {
    json!({ "id": 7, "name": "alice", "secret": "from-client", "token": [1, 2, 3] })
}

/// Echoes what the procedure implementation observed: the ordinary field
/// proves the argument really was decoded, the other two are the ones a
/// client must not be able to set.
macro_rules! observing_procedures {
    () => {
        #[derive(Clone)]
        pub(crate) struct Observe;

        fn observed(account: &cratestack_schema::Account) -> String {
            format!(
                "name={} secret={:?} token={:?}",
                account.name, account.secret, account.token
            )
        }

        impl cratestack_schema::procedures::ProcedureRegistry for Observe {
            async fn inspect_account(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: cratestack_schema::procedures::inspect_account::Args,
                _authorized: cratestack_schema::procedures::inspect_account::Authorized,
            ) -> Result<String, CratestackError> {
                Ok(observed(&args.account))
            }

            async fn inspect_envelope(
                &self,
                _db: &cratestack_schema::Cratestack,
                _ctx: &CratestackContext,
                args: cratestack_schema::procedures::inspect_envelope::Args,
                _authorized: cratestack_schema::procedures::inspect_envelope::Authorized,
            ) -> Result<String, CratestackError> {
                Ok(observed(&args.envelope.account))
            }
        }
    };
}

const NOTHING_LEAKED: &str = r#"name=alice secret="" token=None"#;

async fn call(router: cratestack::axum::Router, path: &str, body: Json) -> String {
    let response = router
        .oneshot(
            Request::post(path)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    assert_eq!(status, StatusCode::OK, "{path}: {text}");
    serde_json::from_slice(&bytes).unwrap_or_else(|_| panic!("{path}: not a string: {text}"))
}

mod rest {
    use super::*;

    include_server_schema!("tests/fixtures/server_only_inbound.cstack", db = Postgres);
    observing_procedures!();

    fn router() -> cratestack::axum::Router {
        let db = cratestack_schema::Cratestack::builder(lazy_pool()).build();
        cratestack_schema::axum::procedure_router(db, Observe, (), JsonCodec, AllowAllAuth)
    }

    #[tokio::test]
    async fn a_model_argument_cannot_set_a_server_only_field() {
        let body = json!({ "account": account_with_masked_values() });
        let seen = call(router(), "/$procs/inspectAccount", body).await;
        assert_eq!(seen, NOTHING_LEAKED);
    }

    #[tokio::test]
    async fn a_model_nested_in_a_type_argument_cannot_set_a_server_only_field() {
        let body = json!({ "envelope": { "account": account_with_masked_values(), "note": "n" } });
        let seen = call(router(), "/$procs/inspectEnvelope", body).await;
        assert_eq!(seen, NOTHING_LEAKED);
    }

    /// Masked means ignored: a wrong-typed value for the field is not a
    /// decode error either, since the key is never read.
    #[test]
    fn the_model_struct_ignores_the_key_whatever_its_type() {
        for secret in [json!("from-client"), json!(5)] {
            let mut body = account_with_masked_values();
            body["secret"] = secret;
            let account: cratestack_schema::Account = serde_json::from_value(body).unwrap();
            assert_eq!((account.secret.as_str(), account.token), ("", None));
        }
    }

    /// And the outbound half still holds, DB-less, so neither direction of
    /// `skip` can be dropped without a test here noticing.
    #[test]
    fn the_model_struct_never_writes_the_key() {
        let account = cratestack_schema::Account {
            id: 7,
            name: "alice".to_owned(),
            secret: "server-set".to_owned(),
            token: Some(vec![1, 2, 3]),
        };
        assert_eq!(
            serde_json::to_value(&account).unwrap(),
            json!({ "id": 7, "name": "alice" })
        );
    }
}

mod rpc {
    use super::*;

    include_server_schema!(
        "tests/fixtures/server_only_inbound_rpc.cstack",
        db = Postgres
    );
    observing_procedures!();

    fn router() -> cratestack::axum::Router {
        let db = cratestack_schema::Cratestack::builder(lazy_pool()).build();
        cratestack_schema::axum::rpc_router(
            db,
            Observe,
            (),
            JsonCodec,
            AllowAllAuth,
            cratestack::DEFAULT_BODY_LIMIT_BYTES,
        )
    }

    #[tokio::test]
    async fn a_model_argument_cannot_set_a_server_only_field() {
        let body = json!({ "account": account_with_masked_values() });
        let seen = call(router(), "/rpc/procedure.inspectAccount", body).await;
        assert_eq!(seen, NOTHING_LEAKED);
    }

    #[tokio::test]
    async fn a_model_nested_in_a_type_argument_cannot_set_a_server_only_field() {
        let body = json!({ "envelope": { "account": account_with_masked_values(), "note": "n" } });
        let seen = call(router(), "/rpc/procedure.inspectEnvelope", body).await;
        assert_eq!(seen, NOTHING_LEAKED);
    }
}
