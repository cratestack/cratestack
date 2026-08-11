//! Resolve secret-bearing config strings.
//!
//! A `studio.toml` value can be a literal, `env:NAME` to read a
//! process env var, or `file:PATH` to read a (trimmed) file body.
//! Plain literals pass through unchanged.

use std::path::PathBuf;

use super::StudioConfigError;

/// Resolve an `env:NAME` or `file:PATH` reference to a literal value.
/// The `field` argument appears in error messages so config-load
/// failures point at the bad `studio.toml` entry instead of the
/// resolved value.
pub fn resolve_secret(value: &str, field: &str) -> Result<String, StudioConfigError> {
    resolve_secret_with(value, field, |name| std::env::var(name))
}

/// Same as [`resolve_secret`], but the `env:` branch goes through
/// `lookup_env` instead of the real process environment.
///
/// This exists so `env:`'s success path can be unit-tested without
/// process-wide env mutation: `std::env::set_var` is `unsafe` as of the
/// 2024 edition (it is not sound against concurrent reads on other
/// threads), and this workspace `forbid`s `unsafe_code` — a test seam
/// is a better trade than being the one place that needs an exemption.
pub(in crate::config) fn resolve_secret_with(
    value: &str,
    field: &str,
    lookup_env: impl Fn(&str) -> Result<String, std::env::VarError>,
) -> Result<String, StudioConfigError> {
    if let Some(name) = value.strip_prefix("env:") {
        lookup_env(name).map_err(|_| StudioConfigError::MissingEnv {
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
