//! Proves procedure-output `@computed`-field composition
//! (docs/design/computed-fields.md's "Procedure outputs" section) reaches
//! `db = None` schemas too, not just `db = Postgres` — the generated
//! `compose_<owner>_value` helpers only ever touch `resolvers`/`ctx`/the
//! owner value itself, never the database, so there's nothing backend-
//! specific about this stage. Mirrors `tests/no_database_procedures.rs`'s
//! own "first-class no-database facade" framing: this crate has no
//! `cratestack-sqlx` dependency under any feature (see `Cargo.toml`/
//! `src/lib.rs`), so a green test here is real evidence the composition
//! path doesn't secretly assume a `Cratestack` backed by a pool.

use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{CratestackContext, CratestackError};
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/computed_fields.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn get_widget(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::get_widget::Args,
        _authorized: cratestack_schema::procedures::get_widget::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::get_widget::Output, CratestackError>,
    > + Send {
        async move { Ok(cratestack_schema::Widget { label: args.label }) }
    }
}

#[derive(Clone)]
struct TestComputedFieldResolver;

impl cratestack_schema::ComputedFieldResolver for TestComputedFieldResolver {
    fn resolve_widget_slug(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::Widget,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<String, CratestackError>> + Send {
        let slug = source.label.to_lowercase().replace(' ', "-");
        async move { Ok(slug) }
    }
}

#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

fn build_router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::router(
        db,
        Procedures,
        TestComputedFieldResolver,
        JsonCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

/// The headline evidence: a `db = None` router — no pool, no
/// `cratestack-sqlx` in the dependency graph at all — composes a
/// procedure's computed-bearing output over real HTTP. Before this
/// stage, `Widget`'s server-side struct only ever carries `label` (the
/// `@computed` field is excluded, docs/design/computed-fields.md), so an
/// un-composed response would be missing `slug` entirely.
#[tokio::test]
async fn no_database_router_composes_a_computed_bearing_procedure_output() {
    let app = build_router();

    let body = serde_json::json!({ "label": "Cool Widget" });
    let response = app
        .oneshot(
            Request::post("/$procs/getWidget")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply["label"], "Cool Widget");
    assert_eq!(reply["slug"], "cool-widget");
}
