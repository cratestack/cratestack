//! Split out of `guards.rs` to stay under the crate's ~200-LoC file
//! convention once the bypass-return-value coverage (cratestack#507
//! finding 3) was added — the same discipline this PR already applied
//! when it carved `guards.rs` out of `records.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use rusqlite::Connection;

use super::*;
use crate::data::sqlite::SqliteSource;

fn target(has_db: bool, allow_unsafe_db_writes: bool, mode: TargetMode) -> LoadedTarget {
    let schema = Arc::new(
        cratestack_parser::parse_schema(
            r#"
            model Versioned {
              id String @id
              version Int @version
            }
            model Emitting {
              id String @id
              @@emit(created)
            }
            model Plain {
              id String @id
              name String
            }
            "#,
        )
        .expect("schema parses"),
    );
    let conn = Connection::open_in_memory().expect("sqlite open");
    LoadedTarget {
        key: "t".to_owned(),
        display_name: "t".to_owned(),
        mode,
        schema: schema.clone(),
        schema_path: PathBuf::from("x.cstack"),
        source: Arc::new(SqliteSource::new(conn, schema)),
        has_db,
        has_api: false,
        allow_unsafe_db_writes,
    }
}

fn model<'a>(target: &'a LoadedTarget, name: &str) -> &'a Model {
    target
        .schema
        .models
        .iter()
        .find(|m| m.name == name)
        .expect("model present")
}

#[test]
fn refuses_versioned_model_on_db_target_without_opt_in() {
    let t = target(true, false, TargetMode::Rw);
    let error = require_safe_write(&t, model(&t, "Versioned")).expect_err("should refuse");
    assert!(matches!(error, ApiError::UnsafeDbWrite { .. }));
}

#[test]
fn refuses_emitting_model_on_db_target_without_opt_in() {
    let t = target(true, false, TargetMode::Rw);
    let error = require_safe_write(&t, model(&t, "Emitting")).expect_err("should refuse");
    assert!(matches!(error, ApiError::UnsafeDbWrite { .. }));
}

#[test]
fn allows_versioned_model_with_opt_in_and_reports_the_bypass() {
    let t = target(true, true, TargetMode::Rw);
    let bypassed = require_safe_write(&t, model(&t, "Versioned")).expect("opted in, should allow");
    assert!(
        bypassed,
        "opted-in write past @version must report itself as a bypass, \
         so the caller can mark the audit entry (cratestack#507 finding 3)"
    );
}

#[test]
fn allows_plain_model_without_opt_in_and_is_not_a_bypass() {
    let t = target(true, false, TargetMode::Rw);
    let bypassed =
        require_safe_write(&t, model(&t, "Plain")).expect("no annotations, should allow");
    assert!(
        !bypassed,
        "a model with neither @version nor @@emit is never a bypass, opt-in or not"
    );
}

#[test]
fn allows_plain_model_with_opt_in_and_is_not_a_bypass() {
    // allow_unsafe_writes = true but the model carries no annotation
    // the flag would ever matter for: it must not be flagged as a
    // bypass just because the target opted in.
    let t = target(true, true, TargetMode::Rw);
    let bypassed =
        require_safe_write(&t, model(&t, "Plain")).expect("no annotations, should allow");
    assert!(!bypassed);
}

#[test]
fn allows_versioned_model_on_api_only_target() {
    let t = target(false, false, TargetMode::Rw);
    let bypassed =
        require_safe_write(&t, model(&t, "Versioned")).expect("api-only target is unaffected");
    assert!(
        !bypassed,
        "an [target.api]-only target never goes through the SQL bypass path"
    );
}
