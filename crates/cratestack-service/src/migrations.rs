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
mod tests;
