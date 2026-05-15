//! Boot-time materialization of the `studio.toml` config into live
//! state: parsed `.cstack` schemas, sqlx pools, and an
//! `Arc<dyn DataSource>` per target.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cratestack_core::Schema;
use sqlx_core::pool::PoolOptions;
use sqlx_postgres::{PgPool, Postgres};

use crate::config::{
    DbDriver, StudioConfig, StudioConfigError, TargetConfig, TargetMode, WorkspaceConfig,
    resolve_secret,
};
use crate::data::DataSource;
use crate::data::api::ApiSource;
use crate::data::postgres::PostgresSource;
use crate::data::sqlite::SqliteSource;

/// In-memory workspace state shared by every request handler.
#[derive(Debug)]
pub struct LoadedWorkspace {
    pub config: WorkspaceConfig,
    pub targets: Vec<Arc<LoadedTarget>>,
}

/// One target with everything resolved: schema, pool, source.
#[derive(Debug)]
pub struct LoadedTarget {
    pub key: String,
    pub display_name: String,
    pub mode: TargetMode,
    pub schema: Arc<Schema>,
    pub schema_path: PathBuf,
    pub source: Arc<dyn DataSource>,
    /// `true` when this target has a `[target.db]` block. Used by the
    /// `/api/targets` capabilities response. Phase 1a always equals
    /// `source` being a [`PostgresSource`].
    pub has_db: bool,
    /// `true` when this target has a `[target.api]` block. May be true
    /// alongside `has_db` once Phase 1b lands the prefer-for routing.
    pub has_api: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error(transparent)]
    Config(#[from] StudioConfigError),
    #[error("failed to read schema '{path}' for target '{key}': {source}")]
    SchemaIo {
        key: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse schema '{path}' for target '{key}':\n{rendered}")]
    SchemaParse {
        key: String,
        path: PathBuf,
        rendered: String,
    },
    #[error("failed to connect target '{key}' to {driver:?}: {source}")]
    Pool {
        key: String,
        driver: DbDriver,
        #[source]
        source: sqlx_core::Error,
    },
    #[error("driver '{driver:?}' is not supported in this Studio build (target '{key}')")]
    UnsupportedDriver { key: String, driver: DbDriver },
    #[error("failed to open SQLite database for target '{key}': {source}")]
    SqliteOpen {
        key: String,
        #[source]
        source: rusqlite::Error,
    },
    #[error("sqlite open task panicked for target '{key}': {message}")]
    SqliteJoin { key: String, message: String },
    #[error("failed to build HTTP client for target '{key}': {source}")]
    HttpClient {
        key: String,
        #[source]
        source: reqwest::Error,
    },
}

/// Open a SQLite connection from a `studio.toml` URL.
///
/// Accepted forms (kept narrow on purpose — SQLite has historically
/// accreted format variants that all mean roughly the same thing):
///
/// - `sqlite:` / `sqlite::memory:` — in-memory database
/// - `sqlite:/path/to/db.sqlite` — file path (leading slash kept)
/// - `sqlite:path/to/db.sqlite` — file path (relative)
/// - Any bare path also works (treated as a file path)
fn open_sqlite(url: &str) -> Result<rusqlite::Connection, rusqlite::Error> {
    let trimmed = url
        .strip_prefix("sqlite://")
        .or_else(|| url.strip_prefix("sqlite:"))
        .unwrap_or(url);
    if trimmed.is_empty() || trimmed == ":memory:" {
        rusqlite::Connection::open_in_memory()
    } else {
        rusqlite::Connection::open(trimmed)
    }
}

impl LoadedWorkspace {
    /// Load the config file, resolve secrets, parse schemas, open
    /// pools, and return a workspace ready for request handling.
    pub async fn load(config_path: &Path) -> Result<Arc<Self>, WorkspaceError> {
        let raw = StudioConfig::load(config_path)?;
        let base_dir = config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let mut targets = Vec::with_capacity(raw.targets.len());
        for target_cfg in &raw.targets {
            targets.push(Arc::new(load_target(target_cfg, &raw, &base_dir).await?));
        }

        Ok(Arc::new(Self {
            config: raw.workspace,
            targets,
        }))
    }

    /// Lookup a target by its config key. Linear scan; the target
    /// count is bounded by the workspace size, which we don't expect
    /// past a few dozen.
    pub fn target(&self, key: &str) -> Option<&Arc<LoadedTarget>> {
        self.targets.iter().find(|t| t.key == key)
    }
}

async fn load_target(
    target: &TargetConfig,
    parent: &StudioConfig,
    base_dir: &Path,
) -> Result<LoadedTarget, WorkspaceError> {
    let schema_path = if target.schema.is_absolute() {
        target.schema.clone()
    } else {
        base_dir.join(&target.schema)
    };

    let schema_text =
        std::fs::read_to_string(&schema_path).map_err(|source| WorkspaceError::SchemaIo {
            key: target.key.clone(),
            path: schema_path.clone(),
            source,
        })?;
    let schema = cratestack_parser::parse_schema(&schema_text).map_err(|error| {
        WorkspaceError::SchemaParse {
            key: target.key.clone(),
            path: schema_path.clone(),
            rendered: error.render(&schema_path.display().to_string(), &schema_text),
        }
    })?;
    let schema = Arc::new(schema);

    let source: Arc<dyn DataSource> = if let Some(db) = &target.db {
        match db.driver {
            DbDriver::Postgres => {
                let url = resolve_secret(
                    &db.url,
                    &format!("target[{}].db.url", target.key),
                )?;
                let pool: PgPool = PoolOptions::<Postgres>::new()
                    .max_connections(db.max_connections.unwrap_or(5))
                    .acquire_timeout(Duration::from_secs(10))
                    .connect(&url)
                    .await
                    .map_err(|source| WorkspaceError::Pool {
                        key: target.key.clone(),
                        driver: DbDriver::Postgres,
                        source,
                    })?;
                Arc::new(PostgresSource::new(pool, schema.clone()))
            }
            DbDriver::Sqlite => {
                let url = resolve_secret(
                    &db.url,
                    &format!("target[{}].db.url", target.key),
                )?;
                let target_key = target.key.clone();
                let conn = tokio::task::spawn_blocking(move || open_sqlite(&url))
                    .await
                    .map_err(|e| WorkspaceError::SqliteJoin {
                        key: target_key.clone(),
                        message: e.to_string(),
                    })?
                    .map_err(|source| WorkspaceError::SqliteOpen {
                        key: target.key.clone(),
                        source,
                    })?;
                Arc::new(SqliteSource::new(conn, schema.clone()))
            }
            other => {
                return Err(WorkspaceError::UnsupportedDriver {
                    key: target.key.clone(),
                    driver: other,
                });
            }
        }
    } else if let Some(api) = &target.api {
        let token_field = format!("target[{}].api.auth.token", target.key);
        let resolved_auth = if let Some(auth) = &api.auth {
            Some(match auth {
                crate::config::ApiAuth::Bearer { token } => crate::config::ApiAuth::Bearer {
                    token: resolve_secret(token, &token_field)?,
                },
                crate::config::ApiAuth::Header { name, value } => {
                    crate::config::ApiAuth::Header {
                        name: name.clone(),
                        value: resolve_secret(
                            value,
                            &format!("target[{}].api.auth.value", target.key),
                        )?,
                    }
                }
            })
        } else {
            None
        };
        Arc::new(
            ApiSource::new(api.base_url.clone(), resolved_auth.as_ref(), schema.clone())
                .map_err(|source| WorkspaceError::HttpClient {
                    key: target.key.clone(),
                    source,
                })?,
        )
    } else {
        unreachable!("StudioConfig::validate rejects targets without db or api");
    };

    let display_name = target
        .display_name
        .clone()
        .unwrap_or_else(|| target.key.clone());

    Ok(LoadedTarget {
        key: target.key.clone(),
        display_name,
        mode: parent.target_mode(target),
        schema,
        schema_path,
        source,
        has_db: target.db.is_some(),
        has_api: target.api.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Studio.toml + a tiny schema, but no DB connection — we drive a
    /// load failure on the pool step to prove the rest of the chain is
    /// wired. Pure-success paths require a real Postgres and are
    /// covered by the testcontainers-gated integration test.
    #[tokio::test]
    async fn load_reports_pool_failure_with_target_key() {
        let temp = tempfile::tempdir().expect("temp dir");
        let schema_path = temp.path().join("user.cstack");
        let mut schema_file = std::fs::File::create(&schema_path).expect("schema file");
        writeln!(
            schema_file,
            "model User {{\n  id String @id\n  name String\n}}"
        )
        .expect("write schema");

        let config_path = temp.path().join("studio.toml");
        std::fs::write(
            &config_path,
            format!(
                r#"
                [workspace]
                name = "smoke"

                [[target]]
                key = "users"
                schema = "{schema}"

                [target.db]
                url = "postgres://nope:nope@127.0.0.1:1/db_does_not_exist"
                driver = "postgres"
                "#,
                schema = schema_path.file_name().unwrap().to_str().unwrap(),
            ),
        )
        .expect("studio.toml writes");

        let error = LoadedWorkspace::load(&config_path)
            .await
            .expect_err("pool connect should fail");
        let message = error.to_string();
        assert!(
            message.contains("target 'users'"),
            "error should name the target, got: {message}"
        );
    }

    #[tokio::test]
    async fn api_only_target_loads_without_a_db() {
        let temp = tempfile::tempdir().expect("temp dir");
        let schema_path = temp.path().join("inv.cstack");
        std::fs::write(
            &schema_path,
            "model Item {\n  id String @id\n  name String\n}",
        )
        .expect("write schema");

        let config_path = temp.path().join("studio.toml");
        std::fs::write(
            &config_path,
            format!(
                r#"
                [[target]]
                key = "inv"
                schema = "{schema}"

                [target.api]
                base_url = "https://inventory.internal"
                "#,
                schema = schema_path.file_name().unwrap().to_str().unwrap(),
            ),
        )
        .expect("studio.toml writes");

        let workspace = LoadedWorkspace::load(&config_path)
            .await
            .expect("api-only load succeeds");
        assert_eq!(workspace.targets.len(), 1);
        assert_eq!(workspace.targets[0].key, "inv");
    }
}
