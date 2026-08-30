//! Forward-only migration runner with a checksum guard against drift.
//! Banks write migrations by hand (the contract under regulation is "the
//! change is reviewable as a SQL diff").

use crate::sqlx;
use cratestack_core::CratestackError;
use sha2::{Digest, Sha256};

pub const MIGRATIONS_TABLE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS cratestack_migrations (
    id TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    checksum BYTEA NOT NULL,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
"#;

/// A single migration step. The runner applies any rows not yet
/// present in `cratestack_migrations`. `down` is recorded but never
/// called — irreversible-by-default is the safe banking posture.
#[derive(Debug, Clone, Default)]
pub struct Migration {
    /// Sortable id, conventionally `YYYYMMDDHHMMSS_<slug>`.
    pub id: String,
    pub description: String,
    /// Preparatory SQL run immediately before [`Self::up`], in the
    /// *same* transaction — the `up.pre.sql` half of a migration
    /// directory, scaffolded by `cratestack migrate diff` whenever it
    /// emits a blocking op and filled in by the operator.
    ///
    /// A separate field rather than text prepended to `up` so ownership
    /// stays clean: `up.sql` is wholly generated, `up.pre.sql` wholly
    /// hand-authored.
    pub up_pre: Option<String>,
    pub up: String,
    pub down: Option<String>,
}

impl Migration {
    pub fn checksum(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.id.as_bytes());
        hasher.update(b"\0");
        hasher.update(self.description.as_bytes());
        hasher.update(b"\0");
        hasher.update(self.up.as_bytes());
        // Mixed in only when present, so a migration without an
        // `up.pre.sql` hashes byte-identically to how it did before
        // `up_pre` existed. Hashing `None` as (say) an empty string
        // plus a separator would change every checksum already
        // recorded in `cratestack_migrations`, and every deployment
        // upgrading to this version would see its entire applied
        // history as `ChecksumMismatch` — drift where nothing drifted.
        if let Some(up_pre) = &self.up_pre {
            hasher.update(b"\0");
            hasher.update(up_pre.as_bytes());
        }
        hasher.finalize().into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStatus {
    Pending,
    Applied,
    ChecksumMismatch,
}

#[derive(Debug, Clone)]
pub struct MigrationState {
    pub id: String,
    pub status: MigrationStatus,
}

pub async fn ensure_migrations_table(pool: &sqlx::PgPool) -> Result<(), CratestackError> {
    // `raw_sql` sends the whole DDL block as one round-trip over PG's
    // simple-query protocol, which understands `;`-separated statements
    // (and dollar-quoting) natively — no client-side splitting needed.
    sqlx::raw_sql(MIGRATIONS_TABLE_DDL)
        .execute(pool)
        .await
        .map_err(|error| CratestackError::Database(error.to_string()))?;
    Ok(())
}

/// Inspect each migration in `migrations` against `cratestack_migrations`
/// and report which are pending / applied / drifted. Use before `apply` to
/// surface drift to the operator without changing state.
pub async fn status(
    pool: &sqlx::PgPool,
    migrations: &[Migration],
) -> Result<Vec<MigrationState>, CratestackError> {
    ensure_migrations_table(pool).await?;
    let rows = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT id, checksum FROM cratestack_migrations ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| CratestackError::Database(error.to_string()))?;

    let mut applied: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    for (id, checksum) in rows {
        applied.insert(id, checksum);
    }

    Ok(migrations
        .iter()
        .map(|m| {
            let id = m.id.clone();
            match applied.get(&id) {
                Some(stored) if stored.as_slice() == m.checksum().as_slice() => MigrationState {
                    id,
                    status: MigrationStatus::Applied,
                },
                Some(_) => MigrationState {
                    id,
                    status: MigrationStatus::ChecksumMismatch,
                },
                None => MigrationState {
                    id,
                    status: MigrationStatus::Pending,
                },
            }
        })
        .collect())
}

/// Apply every pending migration in the input slice in order. Each
/// runs in its own transaction — [`Migration::up_pre`] then
/// [`Migration::up`], both inside it — and checksum drift aborts the
/// whole apply (banks treat drift as a release-process failure for
/// humans, not a silent overwrite).
pub async fn apply_pending(
    pool: &sqlx::PgPool,
    migrations: &[Migration],
) -> Result<Vec<String>, CratestackError> {
    let states = status(pool, migrations).await?;
    for (state, migration) in states.iter().zip(migrations) {
        if state.status == MigrationStatus::ChecksumMismatch {
            return Err(CratestackError::Internal(format!(
                "migration `{}` is recorded as applied but its SQL has changed; \
                 resolve drift before continuing",
                migration.id
            )));
        }
    }

    let mut applied = Vec::new();
    for (state, migration) in states.iter().zip(migrations) {
        if state.status != MigrationStatus::Pending {
            continue;
        }
        let mut tx = pool
            .begin()
            .await
            .map_err(|error| CratestackError::Database(error.to_string()))?;
        // `up.pre.sql` first, in this same transaction. Its purpose is to
        // make `up`'s blocking statement succeed, so a commit boundary
        // between them would defeat it: that window is exactly when a
        // concurrent INSERT could reintroduce the NULL a backfill just
        // removed. Both halves land or neither does.
        if let Some(up_pre) = &migration.up_pre {
            sqlx::raw_sql(sqlx::AssertSqlSafe(up_pre.clone()))
                .execute(&mut *tx)
                .await
                .map_err(|error| CratestackError::Database(error.to_string()))?;
        }
        // `raw_sql` sends the whole `up` script as one batch over PG's
        // simple-query protocol inside this transaction, so a mid-script
        // failure can't leave partial state (and dollar-quoted PL/pgSQL
        // bodies survive intact — no client-side `;` splitting, which
        // would cut inside a `$$...$$` block).
        // `AssertSqlSafe`: `migration.up` *is* SQL by construction — the text
        // of a migration file the operator ships. There is no bind-parameter
        // alternative for a DDL batch (sqlx 0.9's `SqlSafeStr` bound).
        sqlx::raw_sql(sqlx::AssertSqlSafe(migration.up.clone()))
            .execute(&mut *tx)
            .await
            .map_err(|error| CratestackError::Database(error.to_string()))?;
        sqlx::query(
            "INSERT INTO cratestack_migrations (id, description, checksum) VALUES ($1, $2, $3)",
        )
        .bind(&migration.id)
        .bind(&migration.description)
        .bind(migration.checksum().as_slice())
        .execute(&mut *tx)
        .await
        .map_err(|error| CratestackError::Database(error.to_string()))?;
        tx.commit()
            .await
            .map_err(|error| CratestackError::Database(error.to_string()))?;
        applied.push(migration.id.clone());
    }

    Ok(applied)
}

#[cfg(test)]
mod tests;
