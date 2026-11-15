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
fn url_options_backslash_parity_decides_the_refusal() {
    // One trailing backslash would escape the separator: refused.
    let message = bad_request(PgPoolConfig::from_url(&format!(
        "{URL}?options=-c%20x%3Dy%5C"
    )));
    assert!(message.contains("unpaired backslash"), "{message}");
    // An escaped backslash (`\\`) is a complete value: accepted.
    assert!(PgPoolConfig::from_url(&format!("{URL}?options=-c%20x%3Dy%5C%5C")).is_ok());
    // Three is odd again.
    bad_request(PgPoolConfig::from_url(&format!(
        "{URL}?options=-c%20x%3Dy%5C%5C%5C"
    )));
}

#[test]
fn url_options_naming_a_sqlx_owned_setting_are_refused() {
    for options in [
        "-c%20TimeZone%3DUTC",
        "-cdatestyle%3DISO",
        "--client_encoding%3DUTF8",
        "-c%20statement_timeout%3D1%20-c%20application_name%3Dx",
    ] {
        let message = bad_request(PgPoolConfig::from_url(&format!("{URL}?options={options}")));
        assert!(message.contains("sqlx sets itself"), "{options}: {message}");
    }
    // Other settings, and a sqlx-owned name only in a value, are fine.
    for options in [
        "-c%20statement_timeout%3D1000",
        "-c%20search_path%3Dtimezone",
    ] {
        assert!(
            PgPoolConfig::from_url(&format!("{URL}?options={options}")).is_ok(),
            "{options}"
        );
    }
}

/// A server-reported error carrying only a SQLSTATE, for offline tests.
#[derive(Debug)]
struct FakeDbError(&'static str);

impl std::fmt::Display for FakeDbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "fake server error {}", self.0)
    }
}

impl std::error::Error for FakeDbError {}

impl sqlx::error::DatabaseError for FakeDbError {
    fn message(&self) -> &str {
        "fake server error"
    }
    fn code(&self) -> Option<std::borrow::Cow<'_, str>> {
        Some(self.0.into())
    }
    fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        self
    }
    fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
        self
    }
    fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
        self
    }
    fn kind(&self) -> sqlx::error::ErrorKind {
        sqlx::error::ErrorKind::Other
    }
}

fn db_error(code: &'static str) -> sqlx::Error {
    sqlx::Error::Database(Box::new(FakeDbError(code)))
}

#[test]
fn sqlstates_are_classified_without_a_server() {
    for code in ["57P01", "57P02", "57P03", "08000", "08006", "08P01"] {
        let mapped = classify_connect_error(db_error(code));
        assert!(
            matches!(mapped, CratestackError::Unavailable(_)),
            "{code}: {mapped:?}"
        );
    }
    // 55000 (e.g. `allow_connections false`) is not transient: not Unavailable.
    let mapped = classify_connect_error(db_error("55000"));
    assert!(
        !matches!(mapped, CratestackError::Unavailable(_)),
        "{mapped:?}"
    );
    let unknown = bad_request(Err::<(), _>(classify_connect_error(db_error("42704"))));
    assert!(unknown.contains("unknown setting"), "{unknown}");
    let bad_value = bad_request(Err::<(), _>(classify_connect_error(db_error("22023"))));
    assert!(!bad_value.contains("unknown setting"), "{bad_value}");
    bad_request(Err::<(), _>(classify_connect_error(db_error("28P01"))));
    bad_request(Err::<(), _>(classify_connect_error(db_error("3D000"))));
    bad_request(Err::<(), _>(classify_connect_error(db_error("55P02"))));
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
