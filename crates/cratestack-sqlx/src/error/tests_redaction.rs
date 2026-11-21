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

/// A minimal `tracing` subscriber that records every event's fields as text,
/// so a test can read exactly what a call logged. Built on `tracing` alone:
/// the crate has no subscriber dev-dependency.
mod capture {
    use std::fmt::Write as _;
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    #[derive(Clone, Default)]
    pub(super) struct Capture(Arc<Mutex<String>>);

    impl Capture {
        pub(super) fn text(&self) -> String {
            self.0.lock().expect("capture lock").clone()
        }
    }

    struct Fields<'a>(&'a mut String);

    impl Visit for Fields<'_> {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            let _ = write!(self.0, "{}={value:?} ", field.name());
        }
    }

    impl Subscriber for Capture {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, event: &Event<'_>) {
            let mut out = self.0.lock().expect("capture lock");
            let _ = write!(out, "{} ", event.metadata().level());
            event.record(&mut Fields(&mut out));
            out.push('\n');
        }
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }
}

/// The cause of a `Configuration` or `Tls` error (which can hold the URL and
/// password) is not logged either: the error-level event names only the
/// kind of fault.
#[test]
fn configuration_and_tls_causes_never_reach_the_log() {
    let capture = capture::Capture::default();
    tracing::subscriber::with_default(capture.clone(), || {
        let cause = || format!("postgres://u:{SECRET}@db.internal:5432/x");
        let _ = cratestack_error_from_sqlx(sqlx::Error::Configuration(cause().into()));
        let _ = cratestack_error_from_sqlx(sqlx::Error::Tls(cause().into()));
    });
    let log = capture.text();
    assert!(log.contains("kind=\"configuration\""), "{log}");
    assert!(log.contains("kind=\"tls\""), "{log}");
    assert!(!log.contains(SECRET), "password reached the log: {log}");
    assert!(!log.contains("db.internal"), "host reached the log: {log}");
}
