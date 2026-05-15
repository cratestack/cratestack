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
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            name: Self::default_name(),
            default_mode: TargetMode::default(),
            cors_dev: Self::default_cors_dev(),
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TargetDb {
    pub url: String,
    pub driver: DbDriver,
    #[serde(default)]
    pub max_connections: Option<u32>,
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

