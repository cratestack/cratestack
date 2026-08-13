//! `studio.toml` loader and shape.
//!
//! The config carries the workspace header plus zero or more
//! `[[target]]` blocks. Validation rejects duplicate keys, missing
//! channels (`db`/`api`), and URL-unsafe key characters. The actual
//! schema files referenced by each target are loaded by
//! [`crate::workspace::LoadedWorkspace::load`], not here.

mod loader;
mod secrets;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_secrets;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use secrets::resolve_secret;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct StudioConfig {
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    #[serde(rename = "target", default)]
    pub targets: Vec<TargetConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkspaceConfig {
    #[serde(default = "WorkspaceConfig::default_name")]
    pub name: String,
    #[serde(default)]
    pub default_mode: TargetMode,
    /// Permissive CORS for browser-based UI development. Defaults to
    /// `true` because Studio binds 127.0.0.1 — the threat model is "no
    /// public exposure," and a Trunk dev server on `localhost:8080`
    /// needs to call the backend on `localhost:7878`. Set `false` to
    /// disable when binding to a wider interface.
    #[serde(default = "WorkspaceConfig::default_cors_dev")]
    pub cors_dev: bool,
    /// Opt-in path to an append-only JSONL audit sidecar. Relative
    /// paths resolve against the directory holding `studio.toml`.
    ///
    /// Unset (the default) keeps the audit log in process memory only,
    /// which is Studio's zero-footprint posture: it neither writes to
    /// the operator's filesystem nor creates anything in the target
    /// database. Setting it trades that for a log that survives
    /// restarts — see [`crate::audit::AuditLog::persistent`].
    #[serde(default)]
    pub audit_file: Option<PathBuf>,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            name: Self::default_name(),
            default_mode: TargetMode::default(),
            cors_dev: Self::default_cors_dev(),
            audit_file: None,
        }
    }
}

impl WorkspaceConfig {
    fn default_name() -> String {
        "studio".to_owned()
    }
    fn default_cors_dev() -> bool {
        true
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TargetConfig {
    pub key: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub schema: PathBuf,
    #[serde(default)]
    pub mode: Option<TargetMode>,
    #[serde(default)]
    pub db: Option<TargetDb>,
    #[serde(default)]
    pub api: Option<TargetApi>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TargetMode {
    #[default]
    Ro,
    Rw,
}

/// A direct SQL connection Studio opens itself — **not** a proxy to a
/// deployed service. See the crate's top-level rustdoc ("`[target.db]`
/// is not the generated API") for what that means for `@version`,
/// `@@emit`, and `@@allow` (cratestack#507).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TargetDb {
    pub url: String,
    pub driver: DbDriver,
    #[serde(default)]
    pub max_connections: Option<u32>,
    /// Opt-in escape hatch for writing an `@@emit(...)` model straight
    /// through this `[target.db]` connection on a backend that has no
    /// event-outbox equivalent to route it through.
    ///
    /// `@version` bumping is routed for real on every `[target.db]`
    /// backend (Postgres and SQLite alike), so it's never what this flag
    /// gates — see the crate's top-level rustdoc ("`[target.db]` is not
    /// the generated API") for the full per-attribute breakdown. Only
    /// `@@emit(...)` can still be unroutable: `cratestack-rusqlite` has
    /// no `cratestack_event_outbox` table, and `include_embedded_schema!`
    /// itself treats `@@emit(...)` as a no-op on the framework's own
    /// embedded backend, so this is a permanent backend capability
    /// difference, not a gap Studio is expected to close. Left `false`
    /// (the default), Studio refuses `POST` / `PATCH` / `DELETE` against
    /// any `@@emit(...)` model on a non-Postgres `rw` `[target.db]`
    /// target with a `403 UNSAFE_DB_WRITE` naming the attribute, so the
    /// bypass has to be chosen per target rather than discovered after
    /// the fact (cratestack#507, #553). Models with no `@@emit(...)`, and
    /// any model at all on a Postgres target, are unaffected either way.
    /// Setting this to `true` is what chooses the bypass — it does not
    /// change what actually happens on the wire, only whether Studio
    /// will let you do it silently.
    #[serde(default)]
    pub allow_unsafe_writes: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DbDriver {
    Postgres,
    Sqlite,
    Mysql,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TargetApi {
    pub base_url: String,
    #[serde(default)]
    pub auth: Option<ApiAuth>,
    #[serde(default)]
    pub prefer_for: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ApiAuth {
    Bearer { token: String },
    Header { name: String, value: String },
}

#[derive(Debug, thiserror::Error)]
pub enum StudioConfigError {
    #[error("failed to read studio config '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse studio config '{path}': {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("target '{key}' must declare at least one of [target.db] or [target.api]")]
    TargetMissingChannel { key: String },
    #[error("duplicate target key '{key}'")]
    DuplicateKey { key: String },
    #[error("target key '{key}' must be non-empty, url-safe ([A-Za-z0-9_-])")]
    InvalidKey { key: String },
    #[error("env var '{name}' is unset (referenced from {field})")]
    MissingEnv { name: String, field: String },
    #[error("failed to read secret file '{path}' (referenced from {field}): {source}")]
    SecretFile {
        path: PathBuf,
        field: String,
        #[source]
        source: std::io::Error,
    },
}
