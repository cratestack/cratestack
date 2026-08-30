//! Load embedded migrations and apply them at startup.
//!
//! Requires the `postgres` feature (on by default) — this whole module is
//! behind it. [`Migration`]/[`apply_pending`] themselves live in
//! `cratestack-sqlx`, not here; this module only adds the one thing that
//! didn't already exist there — loading a compile-time-embedded
//! `migrations/` directory tree into a `Vec<Migration>` — plus a thin
//! `run_migrations` convenience wrapper over connecting a pool and calling
//! `apply_pending`.

use cratestack_core::CratestackError;
use cratestack_sqlx::sqlx::postgres::PgPoolOptions;
use cratestack_sqlx::{Migration, apply_pending};
use include_dir::Dir;

/// Load every `cratestack migrate diff`-generated migration out of an
/// `include_dir!`-embedded `migrations/postgres` tree, in directory-name
/// (i.e. timestamp) order.
///
/// Each migration lives at
/// `<dir>/<timestamp>_<name>/{up.pre.sql,up.sql,down.sql}`.
///
/// `up.pre.sql` is read if present and runs before `up.sql` in the same
/// transaction — `cratestack migrate diff` scaffolds it whenever it
/// emits a blocking operation, for the operator to fill in with the
/// backfill that makes the blocking statement succeed. `down.sql` is
/// read if present (recorded for operator reference; never executed
/// automatically, matching `apply_pending`'s forward-only-by-design
/// behaviour: see that function's own docs in `cratestack-sqlx`).
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
            // Absent is the common case and means "no preparatory SQL";
            // present-but-blank is what a scaffolded-then-ignored file
            // looks like, and is normalised to `None` so it neither
            // costs a round-trip nor perturbs the checksum.
            let up_pre = migration_dir
                .get_file(migration_dir.path().join("up.pre.sql"))
                .and_then(|file| file.contents_utf8())
                .filter(|contents| !is_effectively_blank(contents))
                .map(str::to_owned);
            let down = migration_dir
                .get_file(migration_dir.path().join("down.sql"))
                .and_then(|file| file.contents_utf8())
                .map(str::to_owned);
            Migration {
                id: id.clone(),
                description: id,
                up_pre,
                up,
                down,
            }
        })
        .collect()
}

/// True when a file carries no executable SQL — only blank lines and
/// `--` line comments.
///
/// `migrate diff` scaffolds `up.pre.sql` as a comment-only TODO block,
/// so the overwhelmingly common state of that file is "generated and
/// never filled in". Treating that as `None` keeps it out of the
/// migration's checksum, which means scaffolding the file for an
/// existing blocking migration does not retroactively invalidate it.
///
/// Deliberately line-oriented: it only has to recognise the shape this
/// crate's own scaffold emits. A file whose only content is a `/* … */`
/// block comment reads as non-blank and is simply sent to the server,
/// which is harmless — the failure mode is one redundant round-trip,
/// not a wrong result.
fn is_effectively_blank(contents: &str) -> bool {
    contents
        .lines()
        .map(str::trim)
        .all(|line| line.is_empty() || line.starts_with("--"))
}

/// Connect a single, non-pooled connection (migrations run once at
/// startup, not on the request path — no pool needed) and apply every
/// pending migration. Returns the ids of the migrations that were
/// actually applied (empty if the schema was already up to date).
pub async fn run_migrations(
    database_url: &str,
    migrations: &[Migration],
) -> Result<Vec<String>, CratestackError> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .map_err(|error| CratestackError::Database(error.to_string()))?;
    apply_pending(&pool, migrations).await
}

#[cfg(test)]
mod tests {
    use include_dir::{Dir, include_dir};

    use super::{Migration, is_effectively_blank, migrations_from_dir};

    static FIXTURE_MIGRATIONS: Dir<'_> =
        include_dir!("$CARGO_MANIFEST_DIR/tests/fixtures/migrations");

    fn migration<'a>(migrations: &'a [Migration], id: &str) -> &'a Migration {
        migrations
            .iter()
            .find(|m| m.id == id)
            .unwrap_or_else(|| panic!("fixture has a `{id}` migration"))
    }

    #[test]
    fn loads_migrations_in_timestamp_order() {
        let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
        let ids: Vec<_> = migrations.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "20260101000000_init",
                "20260102000000_add_index",
                "20260103000000_backfill_owner",
                "20260104000000_scaffold_untouched",
            ]
        );
    }

    /// cratestack#843: before this, an operator could write `up.pre.sql`
    /// exactly as the generated warning instructed and the file would be
    /// silently ignored — the migration then failed at deploy time with
    /// the very NOT NULL violation the backfill was meant to prevent.
    #[test]
    fn reads_up_pre_sql_when_it_has_real_sql() {
        let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
        let backfill = migration(&migrations, "20260103000000_backfill_owner");

        assert_eq!(
            backfill.up_pre.as_deref(),
            Some(
                "-- Backfill so the NOT NULL promotion in up.sql can succeed.\n\
                 UPDATE widgets SET owner = 'unknown' WHERE owner IS NULL;\n"
            )
        );
    }

    /// A scaffold the operator never filled in must be indistinguishable
    /// from no file at all — otherwise scaffolding `up.pre.sql` for an
    /// existing blocking migration would change its checksum and make
    /// every deployment report drift.
    #[test]
    fn comment_only_up_pre_sql_reads_as_absent() {
        let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
        assert_eq!(
            migration(&migrations, "20260104000000_scaffold_untouched").up_pre,
            None
        );
    }

    #[test]
    fn untouched_scaffold_does_not_change_the_checksum() {
        let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
        let scaffolded = migration(&migrations, "20260104000000_scaffold_untouched");

        let mut without_the_file = scaffolded.clone();
        without_the_file.up_pre = None;

        assert_eq!(scaffolded.checksum(), without_the_file.checksum());
    }

    #[test]
    fn absent_up_pre_sql_is_none() {
        let migrations = migrations_from_dir(&FIXTURE_MIGRATIONS);
        assert_eq!(migration(&migrations, "20260101000000_init").up_pre, None);
    }

    #[test]
    fn blankness_is_judged_on_executable_sql_not_emptiness() {
        assert!(is_effectively_blank(""));
        assert!(is_effectively_blank("\n  \n"));
        assert!(is_effectively_blank(
            "-- just a comment\n--   and another\n"
        ));
        assert!(!is_effectively_blank("-- a comment\nUPDATE t SET c = 0;\n"));
        assert!(!is_effectively_blank("UPDATE t SET c = 0; -- trailing\n"));
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
