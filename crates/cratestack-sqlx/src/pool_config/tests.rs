//! Offline tests: URL handling, setting-name validation, option ordering.

use std::time::Duration;

use cratestack_core::CratestackError;

use super::PgPoolConfig;

fn bad_request(result: Result<impl std::fmt::Debug, CratestackError>) -> String {
    match result {
        Err(CratestackError::BadRequest(message)) => message,
        other => panic!("expected BadRequest, got {other:?}"),
    }
}

#[test]
fn from_url_accepts_a_postgres_url() {
    PgPoolConfig::from_url("postgres://u:p@localhost:5432/db").expect("valid url");
    PgPoolConfig::from_url("postgresql://u@localhost/db").expect("valid url");
}

#[test]
fn bad_url_is_bad_request_and_never_echoes_the_password() {
    for url in [
        "not a url",
        "postgres://u:hunter2secret@host:notaport/db",
        "mysql://u:hunter2secret@host/db",
        "postgres://u:hunter2secret@[::1/db",
        "",
    ] {
        let message = bad_request(PgPoolConfig::from_url(url));
        assert!(
            !message.contains("hunter2secret"),
            "message echoed the password: {message}",
        );
    }
}

/// `host=... dbname=...` is refused with a message that says what to use.
#[test]
fn key_value_dsn_is_refused_with_a_clear_message() {
    let message = bad_request(PgPoolConfig::from_url(
        "host=localhost dbname=x password=hunter2secret",
    ));
    assert!(message.contains("key=value"), "{message}");
    assert!(message.contains("postgres://"), "{message}");
    assert!(!message.contains("hunter2secret"), "{message}");
}

#[test]
fn session_settings_are_appended_after_the_url_options() {
    let config =
        PgPoolConfig::from_url("postgres://u@localhost/db?options=-c%20statement_timeout%3D999999")
            .unwrap()
            .session_setting("statement_timeout", "30000")
            .session_setting("lock_timeout", "10000");
    let options = config.connect_options().unwrap();
    let text = options.get_options().expect("options set");
    let url_at = text.find("statement_timeout=999999").expect("url option");
    let ours_at = text.find("statement_timeout=30000").expect("our option");
    assert!(
        ours_at > url_at,
        "ours must come last (server keeps the last -c): {text}"
    );
    assert!(text.contains("-c lock_timeout=10000"), "{text}");
}

#[test]
fn a_setting_given_twice_keeps_the_last_value_once() {
    let config = PgPoolConfig::from_url("postgres://u@localhost/db")
        .unwrap()
        .session_setting("statement_timeout", "1")
        .session_setting("statement_timeout", "2");
    let text = config
        .connect_options()
        .unwrap()
        .get_options()
        .unwrap()
        .to_owned();
    assert_eq!(text.matches("statement_timeout").count(), 1, "{text}");
    assert!(text.contains("statement_timeout=2"), "{text}");
}

#[test]
fn setting_names_are_validated() {
    for good in ["statement_timeout", "_x", "app.tenant_id", "a1.b2"] {
        PgPoolConfig::from_url("postgres://u@localhost/db")
            .unwrap()
            .session_setting(good, "1")
            .connect_options()
            .unwrap_or_else(|e| panic!("{good} should be accepted: {e}"));
    }
    for bad in [
        "",
        "Statement_Timeout",
        "1abc",
        "a b",
        "a=b",
        "a-b",
        "x -c role=admin",
        "a;b",
        "a\u{e9}",
        ".x",
    ] {
        let result = PgPoolConfig::from_url("postgres://u@localhost/db")
            .unwrap()
            .session_setting(bad, "1")
            .connect_options();
        bad_request(result);
    }
}

#[test]
fn setting_values_with_control_characters_are_refused() {
    for bad in ["a\0b", "a\nb"] {
        let result = PgPoolConfig::from_url("postgres://u@localhost/db")
            .unwrap()
            .session_setting("application_name", bad)
            .connect_options();
        bad_request(result);
    }
}

#[tokio::test]
async fn connect_lazy_surfaces_a_deferred_setting_error() {
    let result = PgPoolConfig::from_url("postgres://u@localhost/db")
        .unwrap()
        .session_setting("Bad Name", "1")
        .connect_lazy();
    bad_request(result);
}

#[tokio::test]
async fn zero_max_connections_is_refused() {
    let result = PgPoolConfig::from_url("postgres://u@localhost/db")
        .unwrap()
        .max_connections(0)
        .connect_lazy();
    bad_request(result);
}

#[tokio::test]
async fn pool_bounds_are_applied() {
    let pool = PgPoolConfig::from_url("postgres://u@localhost/db")
        .unwrap()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(7))
        .test_before_acquire(false)
        .connect_lazy()
        .unwrap();
    let options = pool.options();
    assert_eq!(options.get_max_connections(), 2);
    assert_eq!(options.get_acquire_timeout(), Duration::from_secs(7));
    assert!(!options.get_test_before_acquire());
}

#[tokio::test]
async fn defaults_match_the_service_default() {
    let pool = PgPoolConfig::from_url("postgres://u@localhost/db")
        .unwrap()
        .connect_lazy()
        .unwrap();
    assert_eq!(pool.options().get_max_connections(), 5);
}
