//! `StudioConfig::load` / `parse` / `validate` — the read-from-disk
//! and cross-target validation half of the config layer.

use std::fs;
use std::path::{Path, PathBuf};

use super::{StudioConfig, StudioConfigError, TargetConfig, TargetMode};

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
                || target
                    .key
                    .chars()
                    .any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
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
