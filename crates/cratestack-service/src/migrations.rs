//! Load embedded migrations and apply them at startup.
//!
//! Requires the `postgres` feature (on by default) — this whole module is
//! behind it. [`Migration`]/[`apply_pending`] themselves live in
//! `cratestack-sqlx`, not here; this module only adds the one thing that
//! didn't already exist there — loading a compile-time-embedded
//! `migrations/` directory tree into a `Vec<Migration>` — plus a thin
//! `run_migrations` convenience wrapper over connecting a pool and calling
//! `apply_pending`.

use cratestack_core::CoolError;
use cratestack_sqlx::sqlx::postgres::PgPoolOptions;
use cratestack_sqlx::{Migration, apply_pending};
use include_dir::Dir;

/// Load every `cratestack migrate diff`-generated migration out of an
/// `include_dir!`-embedded `migrations/postgres` tree, in directory-name
/// (i.e. timestamp) order.
///
/// Each migration lives at `<dir>/<timestamp>_<name>/{up.sql,down.sql}` —
/// `down.sql` is read if present (recorded for operator reference; never
/// executed automatically, matching `apply_pending`'s forward-only-by-
/// design behaviour: see that function's own docs in `cratestack-sqlx`).
/// Panics on a malformed embedded tree (missing `up.sql`, non-UTF8
/// content) rather than returning a runtime error: this only ever runs
/// against `include_dir!`'s own compile-time-verified output, so a
/// failure here means the crate embedding it is broken, not that a
/// *caller* did something wrong.
pub fn migrations_from_dir(dir: &Dir<'_>) -> Vec<Migration> {
    let mut migration_dirs: Vec<_> = dir.dirs().collect();
    migration_dirs.sort_by_key(|entry| entry.path().to_owned());

    migration_dirs
        .into_iter()
        .map(|migration_dir| {
            let id = migration_dir
                .path()
                .file_name()
                .expect("embedded migration directory should have a name")
                .to_string_lossy()
                .into_owned();
            let up = migration_dir
                .get_file(migration_dir.path().join("up.sql"))
                .unwrap_or_else(|| panic!("migration `{id}` is missing up.sql"))
                .contents_utf8()
                .unwrap_or_else(|| panic!("migration `{id}`'s up.sql is not valid UTF-8"))
                .to_owned();
            let down = migration_dir
                .get_file(migration_dir.path().join("down.sql"))
                .and_then(|file| file.contents_utf8())
                .map(str::to_owned);
            Migration {
                id: id.clone(),
                description: id,
                up,
                down,
            }
        })
        .collect()
}

/// Connect a single, non-pooled connection (migrations run once at
/// startup, not on the request path — no pool needed) and apply every
/// pending migration. Returns the ids of the migrations that were
/// actually applied (empty if the schema was already up to date).
pub async fn run_migrations(
    database_url: &str,
    migrations: &[Migration],
) -> Result<Vec<String>, CoolError> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;
    apply_pending(&pool, migrations).await
}

#[cfg(test)]
mod tests {
    use include_dir::{Dir, include_dir};

    use super::migrations_from_dir;

    static FIXTURE_MIGRATIONS: Dir<'_> =
        include_dir!("$CARGO_MANIFEST_DIR/tests/fixtures/migrations");

    #[test]
    fn loads_migrations_in_timestamp_order() {
        let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
        let ids: Vec<_> = migrations.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["20260101000000_init", "20260102000000_add_index"]);
    }

    #[test]
    fn reads_down_sql_when_present_and_none_when_absent() {
        let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);

        let init = migrations
            .iter()
            .find(|m| m.id == "20260101000000_init")
            .expect("fixture has an init migration");
        assert!(init.up.contains("CREATE TABLE widgets"));
        assert_eq!(init.down.as_deref(), Some("DROP TABLE widgets;\n"));

        let add_index = migrations
            .iter()
            .find(|m| m.id == "20260102000000_add_index")
            .expect("fixture has an add_index migration");
        assert!(add_index.up.contains("CREATE INDEX"));
        assert_eq!(add_index.down, None);
    }

    #[test]
    fn description_defaults_to_the_id() {
        let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
        for migration in &migrations {
            assert_eq!(migration.description, migration.id);
        }
    }
}
