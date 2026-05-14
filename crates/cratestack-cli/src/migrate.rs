//! `cratestack migrate` subcommands.
//!
//! Slice 5 ships `diff`. `verify` (replay against ephemeral DB) and
//! `drift` (introspect live DB) land in subsequent slices.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use cratestack_migrate::{
    EmittedMigration, Snapshot, diff, emit::postgres, emit::sqlite, read_or_empty, write_snapshot,
};

use crate::cli_types::MigrateBackendArg;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Backend {
    Postgres,
    Sqlite,
}

impl Backend {
    fn slug(self) -> &'static str {
        match self {
            Backend::Postgres => "postgres",
            Backend::Sqlite => "sqlite",
        }
    }
}

fn expand(arg: MigrateBackendArg) -> &'static [Backend] {
    match arg {
        MigrateBackendArg::Postgres => &[Backend::Postgres],
        MigrateBackendArg::Sqlite => &[Backend::Sqlite],
        MigrateBackendArg::Both => &[Backend::Postgres, Backend::Sqlite],
    }
}

pub(crate) fn handle_diff(
    schema: PathBuf,
    out_dir: PathBuf,
    backend: MigrateBackendArg,
    name: String,
    allow_destructive: bool,
) -> Result<()> {
    let next_schema = cratestack_parser::parse_schema_file(&schema)
        .map_err(|error| anyhow::anyhow!("{}", crate::cli_support::render_schema_error(&schema, &error)))?;

    let slug = sanitize_slug(&name);
    let timestamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
    let directory_name = format!("{timestamp}_{slug}");

    let backends = expand(backend);
    let mut nothing_to_do = true;

    for backend in backends {
        let backend_dir = out_dir.join(backend.slug());
        let snapshot_path = backend_dir.join("schema.snapshot.json");

        let prev_snapshot = read_or_empty(&snapshot_path)
            .with_context(|| format!("reading snapshot at {}", snapshot_path.display()))?;

        let ops = diff(&prev_snapshot.schema, &next_schema);
        if ops.is_empty() {
            println!(
                "migrate diff [{}]: no changes",
                backend.slug()
            );
            continue;
        }

        let migration = match backend {
            Backend::Postgres => postgres::emit(&ops),
            Backend::Sqlite => sqlite::emit(&ops),
        };

        if migration.has_lossy && !allow_destructive {
            bail!(
                "migrate diff [{}]: refusing to write destructive migration without \
                 --allow-destructive. The diff contains DROP operations that would \
                 destroy data on apply.",
                backend.slug()
            );
        }

        let migration_dir = backend_dir.join(&directory_name);
        write_migration(&migration_dir, &migration)
            .with_context(|| format!("writing migration to {}", migration_dir.display()))?;

        let next_snapshot = Snapshot::from_schema(next_schema.clone());
        write_snapshot(&next_snapshot, &snapshot_path)
            .with_context(|| format!("updating snapshot at {}", snapshot_path.display()))?;

        nothing_to_do = false;
        println!(
            "migrate diff [{}]: wrote {} ({}{}{})",
            backend.slug(),
            migration_dir.display(),
            if migration.has_blocking {
                "blocking"
            } else if migration.has_lossy {
                "lossy"
            } else {
                "safe"
            },
            if migration.has_blocking && migration.has_lossy {
                "+"
            } else {
                ""
            },
            if migration.has_blocking && migration.has_lossy {
                "lossy"
            } else {
                ""
            },
        );
    }

    if nothing_to_do {
        println!("migrate diff: schema is in sync with all selected backends");
    }
    Ok(())
}

fn write_migration(directory: &Path, migration: &EmittedMigration) -> Result<()> {
    fs::create_dir_all(directory)
        .with_context(|| format!("creating {}", directory.display()))?;
    let up_path = directory.join("up.sql");
    let down_path = directory.join("down.sql");
    fs::write(&up_path, &migration.up)
        .with_context(|| format!("writing {}", up_path.display()))?;
    fs::write(&down_path, &migration.down)
        .with_context(|| format!("writing {}", down_path.display()))?;
    Ok(())
}

/// Convert a developer-supplied slug to a filesystem-safe form:
/// lowercase, ASCII alphanumeric + underscore, no leading/trailing
/// underscores. Empty input falls back to "migration".
fn sanitize_slug(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_underscore = false;
    for ch in input.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if ch == '_' || ch == '-' || ch == ' ' {
            Some('_')
        } else {
            None
        };
        if let Some(c) = mapped {
            if c == '_' {
                if prev_underscore || out.is_empty() {
                    continue;
                }
                prev_underscore = true;
            } else {
                prev_underscore = false;
            }
            out.push(c);
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        "migration".to_owned()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn slug_sanitizer_normalizes_developer_input() {
        assert_eq!(sanitize_slug("Add Customer Email"), "add_customer_email");
        assert_eq!(sanitize_slug("--add--col--"), "add_col");
        assert_eq!(sanitize_slug(""), "migration");
        assert_eq!(sanitize_slug("@@!!"), "migration");
        assert_eq!(sanitize_slug("Order #42"), "order_42");
    }

    fn write_schema(dir: &TempDir, source: &str) -> PathBuf {
        let path = dir.path().join("schema.cstack");
        fs::write(&path, source).expect("write schema");
        path
    }

    const INITIAL_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Account {
  id Int @id
  balance Int
}
"#;

    const EXTENDED_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Account {
  id Int @id
  balance Int
  note String?
}
"#;

    #[test]
    fn diff_writes_initial_migration_and_snapshot() {
        let dir = TempDir::new().expect("tempdir");
        let schema = write_schema(&dir, INITIAL_SCHEMA);
        let out = dir.path().join("migrations");

        handle_diff(
            schema,
            out.clone(),
            MigrateBackendArg::Postgres,
            "initial".to_owned(),
            false,
        )
        .expect("diff");

        let backend_dir = out.join("postgres");
        assert!(backend_dir.join("schema.snapshot.json").exists());

        // Exactly one migration directory created.
        let entries: Vec<_> = fs::read_dir(&backend_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect();
        assert_eq!(entries.len(), 1);
        let migration_dir = &entries[0];
        let up = fs::read_to_string(migration_dir.join("up.sql")).unwrap();
        assert!(up.contains("CREATE TABLE accounts"));
    }

    #[test]
    fn second_diff_is_incremental() {
        let dir = TempDir::new().expect("tempdir");
        let schema_path = write_schema(&dir, INITIAL_SCHEMA);
        let out = dir.path().join("migrations");

        handle_diff(
            schema_path.clone(),
            out.clone(),
            MigrateBackendArg::Postgres,
            "initial".to_owned(),
            false,
        )
        .expect("first diff");

        fs::write(&schema_path, EXTENDED_SCHEMA).unwrap();

        handle_diff(
            schema_path,
            out.clone(),
            MigrateBackendArg::Postgres,
            "add_note".to_owned(),
            false,
        )
        .expect("second diff");

        let backend_dir = out.join("postgres");
        let migrations: Vec<_> = fs::read_dir(&backend_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect();
        assert_eq!(migrations.len(), 2);

        // Two diffs run within the same second share a timestamp, so
        // disambiguate by slug rather than relying on sort order.
        let add_note = migrations
            .iter()
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.ends_with("_add_note"))
                    .unwrap_or(false)
            })
            .expect("add_note migration");
        let up = fs::read_to_string(add_note.join("up.sql")).unwrap();
        assert!(up.contains("ALTER TABLE accounts ADD COLUMN note TEXT"));
        assert!(!up.contains("CREATE TABLE"));
    }

    #[test]
    fn destructive_diff_requires_flag() {
        let dir = TempDir::new().expect("tempdir");
        let schema_path = write_schema(&dir, EXTENDED_SCHEMA);
        let out = dir.path().join("migrations");

        handle_diff(
            schema_path.clone(),
            out.clone(),
            MigrateBackendArg::Postgres,
            "initial".to_owned(),
            false,
        )
        .expect("first diff");

        fs::write(&schema_path, INITIAL_SCHEMA).unwrap();

        let result = handle_diff(
            schema_path.clone(),
            out.clone(),
            MigrateBackendArg::Postgres,
            "drop_note".to_owned(),
            false,
        );
        let err = result.expect_err("should refuse destructive without flag");
        assert!(err.to_string().contains("--allow-destructive"));

        // With the flag set, the same diff succeeds.
        handle_diff(
            schema_path,
            out,
            MigrateBackendArg::Postgres,
            "drop_note".to_owned(),
            true,
        )
        .expect("destructive with flag");
    }

    #[test]
    fn both_backends_produce_separate_trees() {
        let dir = TempDir::new().expect("tempdir");
        let schema = write_schema(&dir, INITIAL_SCHEMA);
        let out = dir.path().join("migrations");

        handle_diff(
            schema,
            out.clone(),
            MigrateBackendArg::Both,
            "initial".to_owned(),
            false,
        )
        .expect("both diff");

        assert!(out.join("postgres").join("schema.snapshot.json").exists());
        assert!(out.join("sqlite").join("schema.snapshot.json").exists());

        let pg_entries: Vec<_> = fs::read_dir(out.join("postgres"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect();
        let sqlite_entries: Vec<_> = fs::read_dir(out.join("sqlite"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect();
        assert_eq!(pg_entries.len(), 1);
        assert_eq!(sqlite_entries.len(), 1);

        let pg_up = fs::read_to_string(pg_entries[0].join("up.sql")).unwrap();
        let sqlite_up = fs::read_to_string(sqlite_entries[0].join("up.sql")).unwrap();
        assert!(pg_up.contains("BIGINT"));
        assert!(sqlite_up.contains("BLOB"));
    }
}
