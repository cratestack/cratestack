//! [`resolve_secret`] coverage, split out of `tests.rs` to stay under
//! the crate's ~200-LoC file convention.

use super::*;

#[test]
fn resolve_secret_passes_through_literals() {
    assert_eq!(
        resolve_secret("postgres://localhost/db", "target.db.url").unwrap(),
        "postgres://localhost/db"
    );
}

#[test]
fn resolve_secret_reads_env_var() {
    // SAFETY: process-wide env mutation is acceptable here because each
    // test sets a unique var name and only reads it back synchronously
    // within the same test.
    unsafe { std::env::set_var("STUDIO_TEST_VAR_OK", "from-env") };
    assert_eq!(
        resolve_secret("env:STUDIO_TEST_VAR_OK", "target.db.url").unwrap(),
        "from-env"
    );
}

#[test]
fn resolve_secret_reports_missing_env_with_field() {
    let error = resolve_secret("env:STUDIO_TEST_VAR_MISSING", "target.db.url")
        .expect_err("unset env var should fail");
    match error {
        StudioConfigError::MissingEnv { name, field } => {
            assert_eq!(name, "STUDIO_TEST_VAR_MISSING");
            assert_eq!(field, "target.db.url");
        }
        other => panic!("expected MissingEnv, got {other:?}"),
    }
}

#[test]
fn resolve_secret_reads_file_and_trims() {
    let temp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(temp.path(), "secret-value\n  \n").expect("write");
    let reference = format!("file:{}", temp.path().display());
    assert_eq!(
        resolve_secret(&reference, "target.db.url").unwrap(),
        "secret-value"
    );
}

#[test]
fn resolve_secret_reports_missing_file_with_field() {
    let error = resolve_secret("file:/nonexistent/path-12345", "target.db.url")
        .expect_err("missing file should fail");
    assert!(
        matches!(error, StudioConfigError::SecretFile { ref field, .. } if field == "target.db.url")
    );
}
