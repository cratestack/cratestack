//! `studio.toml` loader and shape.
//!
//! The config is intentionally minimal in Phase 0 — it parses the workspace
//! header and any `[[target]]` blocks but does **not** read the referenced
//! `.cstack` files yet. That happens in Phase 1 when targets start holding
//! live connection state.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            name: Self::default_name(),
            default_mode: TargetMode::default(),
        }
    }
}

impl WorkspaceConfig {
    fn default_name() -> String {
        "studio".to_owned()
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

/// Resolve an `env:NAME` or `file:PATH` reference to a literal value.
/// Plain strings pass through unchanged. The `field` argument is used
/// only in error messages so config-load failures point at the bad
/// `studio.toml` entry instead of the resolved value.
pub fn resolve_secret(value: &str, field: &str) -> Result<String, StudioConfigError> {
    if let Some(name) = value.strip_prefix("env:") {
        std::env::var(name).map_err(|_| StudioConfigError::MissingEnv {
            name: name.to_owned(),
            field: field.to_owned(),
        })
    } else if let Some(path) = value.strip_prefix("file:") {
        std::fs::read_to_string(path)
            .map(|s| s.trim().to_owned())
            .map_err(|source| StudioConfigError::SecretFile {
                path: PathBuf::from(path),
                field: field.to_owned(),
                source,
            })
    } else {
        Ok(value.to_owned())
    }
}

impl StudioConfig {
    /// Parse a `studio.toml` from disk and validate the cross-target rules.
    pub fn load(path: &Path) -> Result<Self, StudioConfigError> {
        let contents = fs::read_to_string(path).map_err(|source| StudioConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&contents).map_err(|source| match source {
            StudioConfigError::Parse { source: e, .. } => StudioConfigError::Parse {
                path: path.to_path_buf(),
                source: e,
            },
            other => other,
        })
    }

    /// Parse from a TOML string and validate. The `path` in any `Parse`
    /// error is set to `studio.toml` — callers that read from disk should
    /// prefer [`StudioConfig::load`] so the real path is surfaced.
    pub fn parse(contents: &str) -> Result<Self, StudioConfigError> {
        let parsed: Self =
            toml::from_str(contents).map_err(|source| StudioConfigError::Parse {
                path: PathBuf::from("studio.toml"),
                source,
            })?;
        parsed.validate()?;
        Ok(parsed)
    }

    fn validate(&self) -> Result<(), StudioConfigError> {
        let mut seen = std::collections::BTreeSet::new();
        for target in &self.targets {
            if target.key.is_empty()
                || target.key.chars().any(|c| {
                    !(c.is_ascii_alphanumeric() || c == '-' || c == '_')
                })
            {
                return Err(StudioConfigError::InvalidKey {
                    key: target.key.clone(),
                });
            }
            if !seen.insert(target.key.clone()) {
                return Err(StudioConfigError::DuplicateKey {
                    key: target.key.clone(),
                });
            }
            if target.db.is_none() && target.api.is_none() {
                return Err(StudioConfigError::TargetMissingChannel {
                    key: target.key.clone(),
                });
            }
        }
        Ok(())
    }

    /// Resolve a target's mode, falling back to the workspace default.
    pub fn target_mode(&self, target: &TargetConfig) -> TargetMode {
        target.mode.unwrap_or(self.workspace.default_mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_config() {
        let cfg = StudioConfig::parse("").expect("empty config is valid");
        assert_eq!(cfg.workspace.name, "studio");
        assert_eq!(cfg.workspace.default_mode, TargetMode::Ro);
        assert!(cfg.targets.is_empty());
    }

    #[test]
    fn parses_workspace_header() {
        let cfg = StudioConfig::parse(
            r#"
                [workspace]
                name = "acme"
                default_mode = "rw"
            "#,
        )
        .expect("workspace header should parse");
        assert_eq!(cfg.workspace.name, "acme");
        assert_eq!(cfg.workspace.default_mode, TargetMode::Rw);
    }

    #[test]
    fn parses_db_target() {
        let cfg = StudioConfig::parse(
            r#"
                [[target]]
                key = "catalog"
                schema = "schemas/catalog.cstack"

                [target.db]
                url = "env:CATALOG_DB_URL"
                driver = "postgres"
                max_connections = 5
            "#,
        )
        .expect("db target should parse");
        let target = &cfg.targets[0];
        assert_eq!(target.key, "catalog");
        let db = target.db.as_ref().expect("db block present");
        assert_eq!(db.driver, DbDriver::Postgres);
        assert_eq!(db.max_connections, Some(5));
        assert_eq!(cfg.target_mode(target), TargetMode::Ro);
    }

    #[test]
    fn parses_api_target_with_bearer_auth() {
        let cfg = StudioConfig::parse(
            r#"
                [[target]]
                key = "accounts"
                schema = "schemas/accounts.cstack"
                mode = "ro"

                [target.api]
                base_url = "https://accounts.internal"
                prefer_for = ["procedures"]
                auth = { kind = "bearer", token = "env:ACCOUNTS_TOKEN" }
            "#,
        )
        .expect("api target should parse");
        let api = cfg.targets[0].api.as_ref().expect("api block present");
        assert_eq!(api.base_url, "https://accounts.internal");
        assert_eq!(api.prefer_for, vec!["procedures".to_owned()]);
        match api.auth.as_ref().expect("auth set") {
            ApiAuth::Bearer { token } => assert_eq!(token, "env:ACCOUNTS_TOKEN"),
            other => panic!("expected bearer auth, got {other:?}"),
        }
    }

    #[test]
    fn rejects_target_without_db_or_api() {
        let error = StudioConfig::parse(
            r#"
                [[target]]
                key = "lonely"
                schema = "schemas/lonely.cstack"
            "#,
        )
        .expect_err("orphaned target should fail validation");
        assert!(matches!(
            error,
            StudioConfigError::TargetMissingChannel { ref key } if key == "lonely"
        ));
    }

    #[test]
    fn rejects_duplicate_keys() {
        let error = StudioConfig::parse(
            r#"
                [[target]]
                key = "dup"
                schema = "a.cstack"
                [target.db]
                url = "sqlite://a.db"
                driver = "sqlite"

                [[target]]
                key = "dup"
                schema = "b.cstack"
                [target.db]
                url = "sqlite://b.db"
                driver = "sqlite"
            "#,
        )
        .expect_err("duplicate keys should fail validation");
        assert!(matches!(
            error,
            StudioConfigError::DuplicateKey { ref key } if key == "dup"
        ));
    }

    #[test]
    fn resolve_secret_passes_through_literals() {
        assert_eq!(
            resolve_secret("postgres://localhost/db", "target.db.url").unwrap(),
            "postgres://localhost/db"
        );
    }

    #[test]
    fn resolve_secret_reads_env_var() {
        // SAFETY: process-wide env mutation is acceptable here because each
        // test sets a unique var name and only reads it back synchronously
        // within the same test.
        unsafe { std::env::set_var("STUDIO_TEST_VAR_OK", "from-env") };
        assert_eq!(
            resolve_secret("env:STUDIO_TEST_VAR_OK", "target.db.url").unwrap(),
            "from-env"
        );
    }

    #[test]
    fn resolve_secret_reports_missing_env_with_field() {
        let error = resolve_secret("env:STUDIO_TEST_VAR_MISSING", "target.db.url")
            .expect_err("unset env var should fail");
        match error {
            StudioConfigError::MissingEnv { name, field } => {
                assert_eq!(name, "STUDIO_TEST_VAR_MISSING");
                assert_eq!(field, "target.db.url");
            }
            other => panic!("expected MissingEnv, got {other:?}"),
        }
    }

    #[test]
    fn resolve_secret_reads_file_and_trims() {
        let temp = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(temp.path(), "secret-value\n  \n").expect("write");
        let reference = format!("file:{}", temp.path().display());
        assert_eq!(
            resolve_secret(&reference, "target.db.url").unwrap(),
            "secret-value"
        );
    }

    #[test]
    fn resolve_secret_reports_missing_file_with_field() {
        let error = resolve_secret("file:/nonexistent/path-12345", "target.db.url")
            .expect_err("missing file should fail");
        assert!(matches!(error, StudioConfigError::SecretFile { ref field, .. } if field == "target.db.url"));
    }

    #[test]
    fn rejects_invalid_key_characters() {
        let error = StudioConfig::parse(
            r#"
                [[target]]
                key = "has spaces"
                schema = "x.cstack"
                [target.db]
                url = "sqlite://x.db"
                driver = "sqlite"
            "#,
        )
        .expect_err("invalid key should fail");
        assert!(matches!(error, StudioConfigError::InvalidKey { .. }));
    }
}
