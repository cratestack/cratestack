//! Boot-time materialization of the `studio.toml` config into live
//! state: parsed `.cstack` schemas, sqlx pools, and an
//! `Arc<dyn DataSource>` per target.
//!
//! [`LoadedWorkspace`] is the facade that every request handler holds
//! by `Arc`. Target loading lives in [`builder`] so the entry stays
//! focused on the workspace lifecycle.

mod builder;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cratestack_core::Schema;

use crate::audit::AuditLog;
use crate::config::{DbDriver, StudioConfig, StudioConfigError, TargetMode, WorkspaceConfig};
use crate::data::DataSource;

/// In-memory workspace state shared by every request handler.
#[derive(Debug)]
pub struct LoadedWorkspace {
    pub config: WorkspaceConfig,
    pub targets: Vec<Arc<LoadedTarget>>,
    pub audit: Arc<AuditLog>,
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
    /// `true` when this target has a `[target.db]` block.
    pub has_db: bool,
    /// `true` when this target has a `[target.api]` block. May be true
    /// alongside `has_db`.
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
/// Accepted forms: `sqlite:` / `sqlite::memory:` for in-memory,
/// `sqlite:/path/...` or `sqlite:path/...` for files, or a bare path.
pub(super) fn open_sqlite(url: &str) -> Result<rusqlite::Connection, rusqlite::Error> {
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
            targets.push(Arc::new(
                builder::load_target(target_cfg, &raw, &base_dir).await?,
            ));
        }

        Ok(Arc::new(Self {
            config: raw.workspace,
            targets,
            audit: Arc::new(AuditLog::new()),
        }))
    }

    /// Lookup a target by its config key. Linear scan; target count is
    /// bounded by the workspace size, which we don't expect past a few
    /// dozen.
    pub fn target(&self, key: &str) -> Option<&Arc<LoadedTarget>> {
        self.targets.iter().find(|t| t.key == key)
    }
}
