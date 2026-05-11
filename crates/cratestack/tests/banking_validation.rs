//! End-to-end test for the `@length`, `@email`, `@regex`, `@iso4217`
//! validation attributes.
//!
//! Verifies that the macro-generated `validate` implementation on the
//! Create/Update inputs fires at the runtime boundary, that the resulting
//! `VALIDATION_ERROR` is surfaced as 422 over HTTP, and — critically —
//! that the public-facing error message never echoes the rejected value
//! (banks would otherwise see PII leak into 4xx logs).

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::sqlx::query;
use cratestack::{AuthProvider, CoolCodec, CoolContext, CoolError, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

include_schema!("tests/fixtures/banking_validation.cstack");

async fn serial_guard() -> tokio::sync::MutexGuard<'static, ()> {
    static M: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    M.lock().await
}

async fn connect_or_skip() -> Option<cratestack::sqlx::PgPool> {
    let database_url = std::env::var("CRATESTACK_TEST_DATABASE_URL").ok()?;
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .ok()
}

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_event_outbox, members")
        .execute(pool)
        .await
        .expect("drop");
    query(
        "CREATE TABLE members (
            id BIGINT PRIMARY KEY,
            email TEXT NOT NULL,
            currency TEXT NOT NULL,
            slug TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create customer");
}

fn ctx() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
}

#[derive(Clone)]
struct PassThroughAuth;

impl AuthProvider for PassThroughAuth {
    type Error = CoolError;
    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        core::future::ready(Ok(ctx()))
    }
}

#[tokio::test]
async fn valid_input_round_trips_through_create() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let created = cool
        .member()
        .create(cratestack_schema::CreateMemberInput {
            id: 1,
            email: "alice@example.com".to_owned(),
            currency: "USD".to_owned(),
            slug: "alice-account".to_owned(),
        })
        .run(&ctx())
        .await
        .expect("valid create");

    assert_eq!(created.email, "alice@example.com");
    assert_eq!(created.currency, "USD");
}

#[tokio::test]
async fn invalid_email_is_rejected_with_validation_error_at_create() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let result = cool
        .member()
        .create(cratestack_schema::CreateMemberInput {
            id: 1,
            email: "not-an-email".to_owned(),
            currency: "USD".to_owned(),
            slug: "alice".to_owned(),
        })
        .run(&ctx())
        .await;

    let err = result.expect_err("invalid email must be rejected");
    assert_eq!(err.code(), "VALIDATION_ERROR");
    let msg = err.public_message().into_owned();
    assert!(
        !msg.contains("not-an-email"),
        "validation error must not echo the rejected value: {msg}",
    );
}

#[tokio::test]
async fn invalid_iso4217_currency_is_rejected() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let result = cool
        .member()
        .create(cratestack_schema::CreateMemberInput {
            id: 1,
            email: "alice@example.com".to_owned(),
            currency: "usd".to_owned(), // lowercase — fails ISO 4217 shape
            slug: "alice".to_owned(),
        })
        .run(&ctx())
        .await;

    let err = result.expect_err("lowercase currency must be rejected");
    assert_eq!(err.code(), "VALIDATION_ERROR");
}

#[tokio::test]
async fn length_below_min_is_rejected() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let result = cool
        .member()
        .create(cratestack_schema::CreateMemberInput {
            id: 1,
            email: "a".to_owned(), // length=1, below min=3 (still passes
            // email shape check after the length
            // check, so order matters: see below)
            currency: "USD".to_owned(),
            slug: "alice".to_owned(),
        })
        .run(&ctx())
        .await;

    let err = result.expect_err("short email must be rejected");
    assert_eq!(err.code(), "VALIDATION_ERROR");
}

#[tokio::test]
async fn regex_violation_is_rejected_for_slug() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let result = cool
        .member()
        .create(cratestack_schema::CreateMemberInput {
            id: 1,
            email: "alice@example.com".to_owned(),
            currency: "USD".to_owned(),
            slug: "Alice!".to_owned(), // uppercase + bang fail ^[a-z0-9-]+$
        })
        .run(&ctx())
        .await;

    let err = result.expect_err("regex violation must be rejected");
    assert_eq!(err.code(), "VALIDATION_ERROR");
}

#[tokio::test]
async fn validation_errors_surface_as_422_over_http_with_redacted_message() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    let cool = cratestack_schema::Cratestack::builder(pool).build();
    let router = cratestack_schema::axum::model_router(cool, JsonCodec, PassThroughAuth);

    let response = router
        .oneshot(
            Request::post("/members")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(
                    r#"{"id":1,"email":"super-secret@bank","currency":"USD","slug":"x"}"#,
                ))
                .expect("req"),
        )
        .await
        .expect("post");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let text = std::str::from_utf8(&bytes).expect("utf8");
    assert!(
        !text.contains("super-secret"),
        "422 body must not echo PII from the rejected value: {text}",
    );
}
