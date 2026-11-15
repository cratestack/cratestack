//! `PgPoolConfig` against a real Postgres (cratestack#1385).
//!
//! Backend selection is the shared `support::pg::connect_or_skip` (URL or
//! testcontainers; `CRATESTACK_REQUIRE_DB` turns a skip into a panic). The URL
//! it hands back carries no query string of its own.

mod support;

use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{CratestackError, PgPoolConfig};

async fn show(pool: &cratestack::sqlx::PgPool, name: &str) -> String {
    cratestack::sqlx::query_scalar::<_, String>(cratestack::sqlx::AssertSqlSafe(format!(
        "SHOW {name}"
    )))
    .fetch_one(pool)
    .await
    .expect("SHOW")
}

fn config_with_hostile_url(url: &str) -> PgPoolConfig {
    PgPoolConfig::from_url(&format!("{url}?options=-c%20statement_timeout%3D999999")).unwrap()
}

#[tokio::test]
async fn session_setting_overrides_the_url_options() {
    let Some(pg) = support::pg::connect_or_skip().await else {
        return;
    };
    let url = pg.url.clone();

    // Control: the URL's own option really is applied without ours, so the
    // override below is not vacuous.
    let control = config_with_hostile_url(&url)
        .max_connections(1)
        .connect_lazy()
        .unwrap();
    assert_ne!(show(&control, "statement_timeout").await, "30s");

    let pool = config_with_hostile_url(&url)
        .max_connections(1)
        .session_setting("statement_timeout", "30000")
        .session_setting("lock_timeout", "10000")
        .connect_lazy()
        .unwrap();
    assert_eq!(show(&pool, "statement_timeout").await, "30s");
    assert_eq!(show(&pool, "lock_timeout").await, "10s");
}

#[tokio::test]
async fn verify_settings_passes_on_a_matching_pool() {
    let Some(pg) = support::pg::connect_or_skip().await else {
        return;
    };
    let url = pg.url.clone();
    let config = config_with_hostile_url(&url)
        .session_setting("statement_timeout", "30000")
        .session_setting("lock_timeout", "10000")
        .application_name("pool_config_test");
    let pool = config.clone().connect_lazy().unwrap();
    config.verify_settings(&pool).await.expect("settings match");
    assert_eq!(show(&pool, "application_name").await, "pool_config_test");

    // The check must leave the connection as it found it.
    assert_eq!(show(&pool, "statement_timeout").await, "30s");
}

/// Expected values are compared as the server renders them, so a different
/// spelling of the same quantity is not a mismatch.
#[tokio::test]
async fn verify_settings_compares_units_not_spellings() {
    let Some(pg) = support::pg::connect_or_skip().await else {
        return;
    };
    let url = pg.url.clone();
    let pool = PgPoolConfig::from_url(&url)
        .unwrap()
        .session_setting("statement_timeout", "30000")
        .connect_lazy()
        .unwrap();
    PgPoolConfig::from_url(&url)
        .unwrap()
        .session_setting("statement_timeout", "30s")
        .verify_settings(&pool)
        .await
        .expect("30000ms is 30s");
}

#[tokio::test]
async fn verify_settings_refuses_a_pool_whose_setting_differs() {
    let Some(pg) = support::pg::connect_or_skip().await else {
        return;
    };
    let url = pg.url.clone();
    // A pool that dropped our startup options, as a transaction-mode pooler
    // would: built directly with a different timeout.
    let options = url
        .parse::<cratestack::sqlx::postgres::PgConnectOptions>()
        .unwrap()
        .options([("statement_timeout", "5000")]);
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy_with(options);

    let config = PgPoolConfig::from_url(&url)
        .unwrap()
        .session_setting("statement_timeout", "30000");
    let error = config
        .verify_settings(&pool)
        .await
        .expect_err("must refuse");
    let CratestackError::Unavailable(message) = &error else {
        panic!("expected Unavailable, got {error:?}");
    };
    assert!(message.contains("statement_timeout"), "{message}");
    assert!(message.contains("30s"), "expected value missing: {message}");
    assert!(message.contains("5s"), "actual value missing: {message}");
    assert!(message.contains("the server reports"), "{message}");
    assert_eq!(error.status_code().as_u16(), 503);

    // The failed check rolled back; the pool is unchanged.
    assert_eq!(show(&pool, "statement_timeout").await, "5s");
}

#[tokio::test]
async fn verify_settings_handles_custom_settings() {
    let Some(pg) = support::pg::connect_or_skip().await else {
        return;
    };
    let url = pg.url.clone();
    let config = PgPoolConfig::from_url(&url)
        .unwrap()
        .session_setting("app.tenant_id", "t-42");
    let pool = config.clone().connect_lazy().unwrap();
    config
        .verify_settings(&pool)
        .await
        .expect("custom GUC set at startup");

    let unset = PgPoolConfig::from_url(&url)
        .unwrap()
        .connect_lazy()
        .unwrap();
    let error = config
        .verify_settings(&unset)
        .await
        .expect_err("not set there");
    assert!(
        matches!(error, CratestackError::Unavailable(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn verify_settings_handles_unset_unknown_and_unchangeable_settings() {
    let Some(pg) = support::pg::connect_or_skip().await else {
        return;
    };
    let url = pg.url.clone();
    let pool = PgPoolConfig::from_url(&url)
        .unwrap()
        .connect_lazy()
        .unwrap();
    let verify = |name: &'static str, value: &'static str| {
        let url = url.clone();
        let pool = pool.clone();
        async move {
            PgPoolConfig::from_url(&url)
                .unwrap()
                .session_setting(name, value)
                .verify_settings(&pool)
                .await
        }
    };

    // Unset custom setting: a mismatch even when "" was expected.
    let error = verify("app.never_set", "").await.expect_err("unset");
    assert!(
        matches!(error, CratestackError::Unavailable(_)),
        "{error:?}"
    );
    // A name the server does not know.
    let error = verify("no_such_setting_xyz", "1")
        .await
        .expect_err("unknown");
    let CratestackError::BadRequest(message) = error else {
        panic!("expected BadRequest");
    };
    assert!(message.contains("unknown setting"), "{message}");
    // Fixed at connection start (backend) and at server start (postmaster).
    for name in ["log_disconnections", "shared_buffers"] {
        let error = verify(name, "on").await.expect_err(name);
        assert!(
            matches!(error, CratestackError::BadRequest(_)),
            "{name}: {error:?}"
        );
        assert_eq!(error.status_code().as_u16(), 400);
    }
}
