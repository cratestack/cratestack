//! A connection error never carries the URL, password or host.

use super::*;

const SECRET: &str = "s3cret";

#[track_caller]
fn assert_no_secret(err: &CratestackError) {
    assert!(!err.to_string().contains(SECRET), "Display: {err}");
    assert!(!format!("{err:?}").contains(SECRET), "Debug: {err:?}");
}

/// A real refused connection from a URL carrying a password: nothing of the
/// URL reaches the mapped error.
#[tokio::test]
async fn refused_connection_error_carries_no_password() {
    let error = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect(&format!("postgres://u:{SECRET}@127.0.0.1:1/x"))
        .await
        .expect_err("port 1 refuses");
    let err = cratestack_error_from_sqlx(error);
    assert!(matches!(err, CratestackError::Unavailable(_)), "{err:?}");
    assert_no_secret(&err);
}

/// A URL that does not parse is a `Configuration` error whose source can
/// echo the URL: it maps to a fixed message.
#[test]
fn unparsable_url_error_carries_no_password() {
    use std::str::FromStr;
    let error = sqlx::postgres::PgConnectOptions::from_str(&format!(
        "postgres://u:{SECRET}@127.0.0.1:notaport/x"
    ))
    .expect_err("bad port");
    let err = cratestack_error_from_sqlx(error);
    assert!(matches!(err, CratestackError::Database(_)), "{err:?}");
    assert_no_secret(&err);
}

/// `Configuration` and `Tls` causes that name the URL or host, built
/// directly so the test does not depend on what sqlx happens to echo.
#[test]
fn configuration_and_tls_causes_never_reach_the_error() {
    let cause = || format!("postgres://u:{SECRET}@db.internal:5432/x");
    let config = sqlx::Error::Configuration(cause().into());
    let tls = sqlx::Error::Tls(cause().into());
    let (config, tls) = (
        cratestack_error_from_sqlx(config),
        cratestack_error_from_sqlx(tls),
    );
    assert_eq!(
        config.to_string(),
        "database: database connection configuration error"
    );
    assert_eq!(tls.to_string(), "database: database TLS error");
    assert_no_secret(&config);
    assert_no_secret(&tls);
}
