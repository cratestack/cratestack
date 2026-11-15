//! `PgPoolConfig` against a real Postgres (cratestack#1385).
//!
//! Needs `CRATESTACK_TEST_DATABASE_URL`; skips without it unless
//! `CRATESTACK_REQUIRE_DB` is set, in which case a missing URL panics.
//! The URL must carry no query string of its own.

use std::time::Duration;

use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{CratestackError, PgPoolConfig};

fn base_url() -> Option<String> {
    match std::env::var("CRATESTACK_TEST_DATABASE_URL") {
        Ok(url) => Some(url),
        Err(_) if std::env::var("CRATESTACK_REQUIRE_DB").is_ok() => {
            panic!("CRATESTACK_REQUIRE_DB is set but CRATESTACK_TEST_DATABASE_URL is not")
        }
        Err(_) => None,
    }
}

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
    let Some(url) = base_url() else { return };

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
    let Some(url) = base_url() else { return };
    let config = config_with_hostile_url(&url)
        .session_setting("statement_timeout", "30000")
        .session_setting("lock_timeout", "10000")
        .session_setting("application_name", "pool_config_test");
    let pool = config.clone().connect_lazy().unwrap();
    config.verify_settings(&pool).await.expect("settings match");

    // The check must leave the connection as it found it.
    assert_eq!(show(&pool, "statement_timeout").await, "30s");
}

/// Expected values are compared as the server renders them, so a different
/// spelling of the same quantity is not a mismatch.
#[tokio::test]
async fn verify_settings_compares_units_not_spellings() {
    let Some(url) = base_url() else { return };
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
    let Some(url) = base_url() else { return };
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
    assert_eq!(error.status_code().as_u16(), 503);

    // The failed check rolled back; the pool is unchanged.
    assert_eq!(show(&pool, "statement_timeout").await, "5s");
}

#[tokio::test]
async fn verify_settings_handles_custom_settings() {
    let Some(url) = base_url() else { return };
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
async fn connect_checks_the_database_is_reachable() {
    let Some(url) = base_url() else { return };
    let pool = PgPoolConfig::from_url(&url)
        .unwrap()
        .connect()
        .await
        .expect("reachable");
    assert_eq!(show(&pool, "server_version_num").await.len(), 6);

    let error = PgPoolConfig::from_url("postgres://u:p@127.0.0.1:1/db")
        .unwrap()
        .acquire_timeout(Duration::from_secs(2))
        .connect()
        .await
        .expect_err("nothing listens on port 1");
    assert!(
        matches!(error, CratestackError::Unavailable(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn bad_url_is_bad_request() {
    let error = PgPoolConfig::from_url("postgres://u:topsecret@host:notaport/db").unwrap_err();
    let CratestackError::BadRequest(message) = error else {
        panic!("expected BadRequest");
    };
    assert!(!message.contains("topsecret"), "{message}");
}

#[tokio::test]
async fn key_value_dsn_is_refused() {
    let error = PgPoolConfig::from_url("host=localhost dbname=x").unwrap_err();
    assert!(matches!(error, CratestackError::BadRequest(_)), "{error:?}");
}

#[tokio::test]
async fn pool_bounds_are_applied() {
    let Some(url) = base_url() else { return };
    let pool = PgPoolConfig::from_url(&url)
        .unwrap()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(3))
        .test_before_acquire(true)
        .connect_lazy()
        .unwrap();
    assert_eq!(pool.options().get_max_connections(), 2);
    assert_eq!(pool.options().get_acquire_timeout(), Duration::from_secs(3));
    assert!(pool.options().get_test_before_acquire());
}
