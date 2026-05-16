//! Forward-only migration runner with a checksum guard against drift.
//! Banks write migrations by hand (the contract under regulation is "the
//! change is reviewable as a SQL diff").

use crate::sqlx;
use cratestack_core::CoolError;
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
#[derive(Debug, Clone)]
pub struct Migration {
    /// Sortable id, conventionally `YYYYMMDDHHMMSS_<slug>`.
    pub id: String,
    pub description: String,
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

pub async fn ensure_migrations_table(pool: &sqlx::PgPool) -> Result<(), CoolError> {
    for statement in MIGRATIONS_TABLE_DDL
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
    }
    Ok(())
}

/// Inspect each migration in `migrations` against `cratestack_migrations`
/// and report which are pending / applied / drifted. Use before `apply` to
/// surface drift to the operator without changing state.
pub async fn status(
    pool: &sqlx::PgPool,
    migrations: &[Migration],
) -> Result<Vec<MigrationState>, CoolError> {
    ensure_migrations_table(pool).await?;
    let rows = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT id, checksum FROM cratestack_migrations ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| CoolError::Database(error.to_string()))?;

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
/// runs in its own transaction; checksum drift aborts the whole apply
/// (banks treat drift as a release-process failure for humans, not a
/// silent overwrite).
pub async fn apply_pending(
    pool: &sqlx::PgPool,
    migrations: &[Migration],
) -> Result<Vec<String>, CoolError> {
    let states = status(pool, migrations).await?;
    for (state, migration) in states.iter().zip(migrations) {
        if state.status == MigrationStatus::ChecksumMismatch {
            return Err(CoolError::Internal(format!(
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
            .map_err(|error| CoolError::Database(error.to_string()))?;
        // PG prepared statements only carry one command per round-trip.
        // Split on `;` inside the migration's transaction so partial
        // state can't survive a mid-script failure (audit/idempotency
        // DDL helpers do the same).
        for statement in migration
            .up
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            sqlx::query(statement)
                .execute(&mut *tx)
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
        }
        sqlx::query(
            "INSERT INTO cratestack_migrations (id, description, checksum) VALUES ($1, $2, $3)",
        )
        .bind(&migration.id)
        .bind(&migration.description)
        .bind(migration.checksum().as_slice())
        .execute(&mut *tx)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;
        tx.commit()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        applied.push(migration.id.clone());
    }

    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migration(id: &str, up: &str) -> Migration {
        Migration {
            id: id.to_owned(),
            description: format!("migration {id}"),
            up: up.to_owned(),
            down: None,
        }
    }

    #[test]
    fn checksum_changes_when_up_sql_changes() {
        let a = migration("20260101000000_init", "CREATE TABLE a (id INT);");
        let mut b = a.clone();
        b.up = "CREATE TABLE a (id BIGINT);".to_owned();
        assert_ne!(a.checksum(), b.checksum());
    }

    #[test]
    fn checksum_is_stable_for_same_inputs() {
        let a = migration("20260101000000_init", "CREATE TABLE a (id INT);");
        let b = a.clone();
        assert_eq!(a.checksum(), b.checksum());
    }
}
