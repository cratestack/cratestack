//! `PgPoolConfig::connect`, URL handling and pool bounds against a real
//! Postgres (cratestack#1385). See `pg_pool_config.rs` for session settings.

use std::time::Duration;

mod support;

use cratestack::{CratestackError, PgPoolConfig};

async fn show(pool: &cratestack::sqlx::PgPool, name: &str) -> String {
    cratestack::sqlx::query_scalar::<_, String>(cratestack::sqlx::AssertSqlSafe(format!(
        "SHOW {name}"
    )))
    .fetch_one(pool)
    .await
    .expect("SHOW")
}

/// `url` with the database name swapped.
fn with_database(url: &str, database: &str) -> String {
    let (head, _) = url.rsplit_once('/').expect("url has a path");
    format!("{head}/{database}")
}

/// `url` with the role swapped (no password).
fn with_role(url: &str, role: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap();
    let (_, host) = rest.split_once('@').expect("url has userinfo");
    format!("{scheme}://{role}@{host}")
}

async fn connect_error(config: PgPoolConfig) -> CratestackError {
    config
        .acquire_timeout(Duration::from_secs(5))
        .connect()
        .await
        .expect_err("connect must fail")
}

fn assert_bad_request(error: &CratestackError, url: &str) {
    let CratestackError::BadRequest(message) = error else {
        panic!("expected BadRequest, got {error:?}");
    };
    assert!(
        !message.contains(url) && !message.contains("127.0.0.1"),
        "{message}"
    );
}

#[tokio::test]
async fn configuration_faults_at_connect_are_bad_request() {
    let Some(pg) = support::pg::connect_or_skip().await else {
        return;
    };
    let url = pg.url.clone();
    let from = |u: &str| PgPoolConfig::from_url(u).unwrap();

    let unknown_db = connect_error(from(&with_database(&url, "no_such_db_xyz"))).await;
    assert_bad_request(&unknown_db, &url);
    let unknown_role = connect_error(from(&with_role(&url, "no_such_role_xyz"))).await;
    assert_bad_request(&unknown_role, &url);
    let bogus_guc = connect_error(from(&format!("{url}?options=-c%20bogus_guc%3D1"))).await;
    assert_bad_request(&bogus_guc, &url);
    let bad_value = connect_error(from(&url).session_setting("statement_timeout", "abc")).await;
    assert_bad_request(&bad_value, &url);
    // A postmaster-context setting cannot be a session setting at all.
    let postmaster = connect_error(from(&url).session_setting("shared_buffers", "1MB")).await;
    assert_bad_request(&postmaster, &url);
}

#[tokio::test]
async fn an_unreachable_port_is_unavailable() {
    let error =
        connect_error(PgPoolConfig::from_url("postgres://u:p@127.0.0.1:1/db").unwrap()).await;
    assert!(
        matches!(error, CratestackError::Unavailable(_)),
        "{error:?}"
    );
    assert_eq!(error.status_code().as_u16(), 503);
}

#[tokio::test]
async fn connect_checks_the_database_is_reachable() {
    let Some(pg) = support::pg::connect_or_skip().await else {
        return;
    };
    let url = pg.url.clone();
    let pool = PgPoolConfig::from_url(&url)
        .unwrap()
        .connect()
        .await
        .expect("reachable");
    assert_eq!(show(&pool, "server_version_num").await.len(), 6);
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
    let Some(pg) = support::pg::connect_or_skip().await else {
        return;
    };
    let url = pg.url.clone();
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
