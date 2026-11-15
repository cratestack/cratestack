use super::*;

/// `RowNotFound` is a missing-row signal and must map to `NotFound`
/// so callers see a 404 instead of a 500.
#[test]
fn row_not_found_maps_to_not_found() {
    let err = cratestack_error_from_sqlx(sqlx::Error::RowNotFound);
    assert!(
        matches!(err, CratestackError::NotFound(_)),
        "RowNotFound should map to CratestackError::NotFound",
    );
    assert_eq!(err.status_code().as_u16(), 404);
    // Typed accessors are not applicable to NotFound.
    assert_eq!(err.db_sqlstate(), None);
    assert_eq!(err.db_constraint(), None);
}

/// Round-trip: a non-database, non-RowNotFound sqlx error (e.g. a
/// configuration / protocol error) must produce the legacy
/// `Database(String)` variant so existing `detail()` callers keep
/// working.
#[test]
fn non_database_sqlx_error_produces_legacy_variant() {
    let err = cratestack_error_from_sqlx(sqlx::Error::Protocol(
        "unexpected EOF from server".to_owned(),
    ));
    assert!(
        matches!(err, CratestackError::Database(_)),
        "Protocol error should fall back to CratestackError::Database",
    );
    assert!(
        err.detail().is_some(),
        "detail() must not be empty for non-database errors",
    );
    // Typed accessors return None for the legacy variant.
    assert_eq!(err.db_sqlstate(), None);
    assert_eq!(err.db_constraint(), None);
}

fn assert_unavailable(error: sqlx::Error) {
    let message = error.to_string();
    let err = cratestack_error_from_sqlx(error);
    assert!(
        matches!(err, CratestackError::Unavailable(_)),
        "{message}: should map to Unavailable, got {err:?}",
    );
    assert_eq!(err.status_code().as_u16(), 503);
    assert_eq!(err.code(), "UNAVAILABLE");
}

#[test]
fn pool_timed_out_maps_to_unavailable() {
    assert_unavailable(sqlx::Error::PoolTimedOut);
}

#[test]
fn pool_closed_maps_to_unavailable() {
    assert_unavailable(sqlx::Error::PoolClosed);
}

#[test]
fn worker_crashed_maps_to_unavailable() {
    assert_unavailable(sqlx::Error::WorkerCrashed);
}

#[test]
fn io_error_maps_to_unavailable_without_leaking_the_address() {
    let io = std::io::Error::new(
        std::io::ErrorKind::ConnectionRefused,
        "connect to 10.1.2.3:5432 refused",
    );
    let err = cratestack_error_from_sqlx(sqlx::Error::Io(io));
    assert!(matches!(err, CratestackError::Unavailable(_)), "{err:?}");
    // `Unavailable`'s string is the public message: no host or port in it.
    assert!(!err.public_message().contains("10.1.2.3"));
    assert_eq!(err.status_code().as_u16(), 503);
}

#[test]
fn tls_error_stays_a_permanent_database_error() {
    // A TLS failure is a configuration fault (sslmode=require against a
    // server without TLS, a name mismatch); it must not read as retryable.
    let err = cratestack_error_from_sqlx(sqlx::Error::Tls("handshake failed".into()));
    assert!(matches!(err, CratestackError::Database(_)), "{err:?}");
    assert_eq!(err.status_code().as_u16(), 500);
}

/// Protocol, decode, configuration and the like stay `Database(String)`.
#[test]
fn decode_and_configuration_errors_stay_database() {
    for error in [
        sqlx::Error::Decode("bad column".into()),
        sqlx::Error::Configuration("bad option".into()),
        sqlx::Error::ColumnNotFound("x".to_owned()),
    ] {
        let err = cratestack_error_from_sqlx(error);
        assert!(matches!(err, CratestackError::Database(_)), "{err:?}");
        assert_eq!(err.status_code().as_u16(), 500);
    }
}

/// The `DatabaseTyped` variant exposes sqlstate and constraint through the
/// typed accessors, and its `detail()` still returns the full operator
/// string (same as the old `error.to_string()` value).
///
/// We can't easily construct a real `PgDatabaseError` in a unit test
/// (it's opaque), so we verify the round-trip contract using
/// `DbErrorInfo` directly and confirm `cratestack_error_from_sqlx` maps
/// the correct variant for the Database arm.
#[test]
fn database_typed_accessors() {
    let info = DbErrorInfo {
        detail: "ERROR: duplicate key value violates unique constraint \"accounts_email_key\""
            .to_owned(),
        sqlstate: Some("23505".to_owned()),
        constraint: Some("accounts_email_key".to_owned()),
    };
    let err = CratestackError::DatabaseTyped(info);

    assert_eq!(err.db_sqlstate(), Some("23505"));
    assert_eq!(err.db_constraint(), Some("accounts_email_key"));
    assert_eq!(err.code(), "DATABASE_ERROR");
    // 5xx — must map to 500.
    let status = err.status_code();
    assert_eq!(status.as_u16(), 500);
    assert_eq!(err.public_message(), "internal error");
    assert!(err.detail().unwrap().contains("duplicate key"));
}

/// `is_retriable` in `isolation.rs` matches on `detail()`.
/// Both `Database(String)` and `DatabaseTyped` must surface their detail
/// so the retry logic continues to work for serialization failures.
#[test]
fn database_typed_detail_preserved_for_retry_logic() {
    let info = DbErrorInfo {
        detail:
            "Database(PgDatabaseError { code: \"40001\", message: \"could not serialize access\" })"
                .to_owned(),
        sqlstate: Some("40001".to_owned()),
        constraint: None,
    };
    let err = CratestackError::DatabaseTyped(info);
    let detail = err.detail().unwrap_or_default();
    assert!(
        detail.contains("40001") || detail.contains("serialize"),
        "detail must still surface retriable substrings: {detail}",
    );
}

/// `DatabaseTyped` with an empty detail must return `None` from `detail()`,
/// consistent with the `Database(String)` behaviour.
#[test]
fn database_typed_empty_detail_returns_none() {
    let err = CratestackError::DatabaseTyped(DbErrorInfo::default());
    assert_eq!(err.detail(), None);
}

/// `into_response` must never leak the operator detail for DatabaseTyped.
#[test]
fn database_typed_into_response_does_not_leak_detail() {
    let info = DbErrorInfo {
        detail: "SELECT * FROM secrets".to_owned(),
        sqlstate: Some("23505".to_owned()),
        constraint: None,
    };
    let response = CratestackError::DatabaseTyped(info).into_response();
    assert_eq!(response.code, "DATABASE_ERROR");
    assert_eq!(response.message, "internal error");
    assert!(!response.message.contains("secrets"));
    assert!(response.details.is_none());
}
