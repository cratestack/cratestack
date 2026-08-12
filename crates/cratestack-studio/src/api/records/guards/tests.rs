//! Split out of `guards.rs` to stay under the crate's ~200-LoC file
//! convention. Covers `require_write_mode`'s decision table
//! (cratestack#507's "option 3" — see that function's doc comment):
//! `@version` alone is always `Routed`; `@@emit` is `Routed` only on a
//! backend with an event outbox (Postgres — `SqliteSource` never
//! reports one, so these unit tests, all `SqliteSource`-backed, cover
//! the "can't route `@@emit`" half of the table). See
//! `tests/postgres_routed_writes.rs` for the Postgres-routed half
//! against a live database.

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
            model VersionedAndEmitting {
              id String @id
              version Int @version
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
fn version_only_is_always_routed_no_opt_in_needed() {
    let t = target(true, false, TargetMode::Rw);
    let mode = require_write_mode(&t, model(&t, "Versioned")).expect("should route");
    assert_eq!(
        mode,
        WriteMode::Routed,
        "@version alone is routable on every backend"
    );
}

#[test]
fn emitting_model_on_a_backend_without_an_outbox_is_refused_without_opt_in() {
    let t = target(true, false, TargetMode::Rw);
    let error = require_write_mode(&t, model(&t, "Emitting")).expect_err("should refuse");
    assert!(matches!(error, ApiError::UnsafeDbWrite { .. }));
}

#[test]
fn emitting_model_on_a_backend_without_an_outbox_bypasses_with_opt_in() {
    let t = target(true, true, TargetMode::Rw);
    let mode = require_write_mode(&t, model(&t, "Emitting")).expect("opted in, should allow");
    assert_eq!(mode, WriteMode::Bypassed);
}

#[test]
fn versioned_and_emitting_is_refused_as_a_whole_when_emit_cannot_route() {
    // `@version` alone would be routable, but the write isn't split —
    // if any annotation can't be routed, the whole write is refused
    // (or bypassed) rather than silently applying half of it. See
    // `WriteMode::Bypassed`'s doc comment.
    let t = target(true, false, TargetMode::Rw);
    let error =
        require_write_mode(&t, model(&t, "VersionedAndEmitting")).expect_err("should refuse");
    assert!(matches!(error, ApiError::UnsafeDbWrite { .. }));
}

#[test]
fn allows_plain_model_without_opt_in() {
    let t = target(true, false, TargetMode::Rw);
    let mode = require_write_mode(&t, model(&t, "Plain")).expect("no annotations, should allow");
    assert_eq!(mode, WriteMode::Plain);
}

#[test]
fn allows_plain_model_with_opt_in() {
    // allow_unsafe_writes = true but the model carries no annotation
    // the flag would ever matter for: it must still report `Plain`,
    // not `Bypassed` — there's nothing being bypassed.
    let t = target(true, true, TargetMode::Rw);
    let mode = require_write_mode(&t, model(&t, "Plain")).expect("no annotations, should allow");
    assert_eq!(mode, WriteMode::Plain);
}

#[test]
fn versioned_model_on_api_only_target_is_plain() {
    let t = target(false, false, TargetMode::Rw);
    let mode =
        require_write_mode(&t, model(&t, "Versioned")).expect("api-only target is unaffected");
    assert_eq!(
        mode,
        WriteMode::Plain,
        "an [target.api]-only target never goes through the SQL routing/bypass path"
    );
}
