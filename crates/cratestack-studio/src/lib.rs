//! CrateStack Studio — admin and testing surface for `.cstack` schemas.
//!
//! Phase 0 surface: `init` writes a starter `studio.toml`, `run` boots an
//! Axum server that serves a stub page. The full data layer, UI, and API
//! land in subsequent phases (see workspace plan).

pub mod config;
pub mod server;

pub use config::{StudioConfig, StudioConfigError, TargetConfig, TargetMode};
pub use server::{ServerError, ServerOptions, run};

/// Default address the studio binds when no override is provided.
pub const DEFAULT_BIND: &str = "127.0.0.1:7878";

/// Default config file name resolved relative to the current directory.
pub const DEFAULT_CONFIG_FILE: &str = "studio.toml";

/// Default starter `studio.toml` body written by `studio init`.
pub const STARTER_CONFIG: &str = include_str!("../starter/studio.toml");
