//! Pre-flight checks for mutation handlers ([`super::writes`]) — run
//! before the data source is touched, so a rejected write never issues
//! any SQL.

use cratestack_core::Model;

use crate::api::ApiError;
use crate::config::TargetMode;
use crate::workspace::LoadedTarget;

/// Reject mutation requests against read-only targets at the earliest
/// point — before we touch the data source.
pub(in crate::api::records) fn require_writable(target: &LoadedTarget) -> Result<(), ApiError> {
    if matches!(target.mode, TargetMode::Rw) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

/// Refuse to write a `@version`/`@@emit` model straight to SQL on a
/// `[target.db]` target unless the target opted in via
/// `allow_unsafe_writes` (cratestack#507).
///
/// A `[target.db]` connection talks raw SQL, not the descriptor path the
/// generated server runs: it never bumps `@version` columns and never
/// writes `cratestack_event_outbox` rows for `@@emit`-annotated models.
/// Both omissions previously landed silently — a `200` with no signal
/// that optimistic concurrency or the event outbox didn't apply. This
/// makes the bypass something an operator chooses per target rather
/// than discovers after the fact. Targets reached only through
/// `[target.api]` are unaffected: those writes go through the deployed
/// service's generated routes, which already apply `@version`/`@@emit`
/// (and `@@allow`) themselves.
pub(in crate::api::records) fn require_safe_write(
    target: &LoadedTarget,
    model_decl: &Model,
) -> Result<(), ApiError> {
    if !target.has_db || target.allow_unsafe_db_writes {
        return Ok(());
    }

    let mut annotations = Vec::new();
    if model_decl
        .fields
        .iter()
        .any(|f| f.attributes.iter().any(|a| a.raw == "@version"))
    {
        annotations.push("@version");
    }
    if model_decl
        .attributes
        .iter()
        .any(|a| a.raw.starts_with("@@emit("))
    {
        annotations.push("@@emit(...)");
    }

    if annotations.is_empty() {
        Ok(())
    } else {
        Err(ApiError::UnsafeDbWrite {
            target: target.key.clone(),
            model: model_decl.name.clone(),
            annotations: annotations.join(" and "),
        })
    }
}

#[cfg(test)]
mod tests {
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
    fn allows_versioned_model_with_opt_in() {
        let t = target(true, true, TargetMode::Rw);
        require_safe_write(&t, model(&t, "Versioned")).expect("opted in, should allow");
    }

    #[test]
    fn allows_plain_model_without_opt_in() {
        let t = target(true, false, TargetMode::Rw);
        require_safe_write(&t, model(&t, "Plain")).expect("no annotations, should allow");
    }

    #[test]
    fn allows_versioned_model_on_api_only_target() {
        let t = target(false, false, TargetMode::Rw);
        require_safe_write(&t, model(&t, "Versioned")).expect("api-only target is unaffected");
    }
}
