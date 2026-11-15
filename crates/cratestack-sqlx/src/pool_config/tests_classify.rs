//! Offline tests: sqlx-owned settings, `application_name`, URL edge cases and
//! the connect-error classes that need no server.

use cratestack_core::CratestackError;

use super::PgPoolConfig;
use super::connect::classify_connect_error;
use crate::sqlx;

const URL: &str = "postgres://u@localhost/db";

fn bad_request(result: Result<impl std::fmt::Debug, CratestackError>) -> String {
    match result {
        Err(CratestackError::BadRequest(message)) => message,
        other => panic!("expected BadRequest, got {other:?}"),
    }
}

#[test]
fn sqlx_owned_settings_are_refused_in_any_case() {
    for name in [
        "datestyle",
        "DateStyle",
        "CLIENT_ENCODING",
        "timezone",
        "TimeZone",
        "extra_float_digits",
        "application_name",
        "Application_Name",
    ] {
        let message = bad_request(
            PgPoolConfig::from_url(URL)
                .unwrap()
                .session_setting(name, "x")
                .connect_options(),
        );
        assert!(message.contains(name), "{message}");
        assert!(message.contains("sqlx sets it at startup"), "{message}");
    }
}

#[test]
fn application_name_setter_uses_the_sqlx_option() {
    let options = PgPoolConfig::from_url(URL)
        .unwrap()
        .application_name("my svc")
        .connect_options()
        .unwrap();
    assert_eq!(options.get_application_name(), Some("my svc"));
    bad_request(
        PgPoolConfig::from_url(URL)
            .unwrap()
            .application_name("a\nb")
            .connect_options(),
    );
}

#[test]
fn url_options_ending_in_a_backslash_are_refused() {
    let message = bad_request(PgPoolConfig::from_url(
        "postgres://u:hunter2secret@localhost/db?options=-c%20x%3D1%5C",
    ));
    assert!(message.contains("backslash"), "{message}");
    assert!(!message.contains("hunter2secret"), "{message}");
}

#[test]
fn connect_errors_are_classified_without_a_server() {
    let io = || sqlx::Error::Io(std::io::Error::other("10.1.2.3:5432"));
    for error in [
        io(),
        sqlx::Error::PoolTimedOut,
        sqlx::Error::PoolClosed,
        sqlx::Error::WorkerCrashed,
    ] {
        let mapped = classify_connect_error(error);
        assert!(
            matches!(mapped, CratestackError::Unavailable(_)),
            "{mapped:?}"
        );
        assert!(!mapped.public_message().contains("10.1.2.3"));
    }
    for error in [
        sqlx::Error::Tls("certificate name mismatch".into()),
        sqlx::Error::Configuration("bad option".into()),
    ] {
        let message = bad_request(Err::<(), _>(classify_connect_error(error)));
        assert!(!message.contains("certificate"), "{message}");
    }
}
